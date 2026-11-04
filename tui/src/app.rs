//! Application state and ncmpcpp key handling. Screens: F1 help, 1 playlist, 2 browser,
//! 3 search engine, 4 media library, 5 playlist editor, l lyrics.
use crate::api::{lyrics_key, Api, Lyrics};
use crate::model::*;
use crate::rpc::Rpc;
use anyhow::Result;
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Help,
    Playlist,
    Browser,
    Search,
    Library,
    Editor,
    Lyrics,
}

/// NC::Menu: a cursor over items, classic (non-centred) scrolling, per-item selection.
pub struct Menu<T> {
    pub items: Vec<T>,
    pub cursor: usize,
    pub offset: usize,
    pub height: usize,
    pub sel: Vec<bool>,
}

impl<T> Default for Menu<T> {
    fn default() -> Self {
        Menu {
            items: vec![],
            cursor: 0,
            offset: 0,
            height: 1,
            sel: vec![],
        }
    }
}

impl<T> Menu<T> {
    pub fn set(&mut self, items: Vec<T>) {
        self.sel = vec![false; items.len()];
        self.items = items;
        self.clamp();
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn cur(&self) -> Option<&T> {
        self.items.get(self.cursor)
    }
    pub fn clamp(&mut self) {
        self.cursor = self.cursor.min(self.len().saturating_sub(1));
        self.sel.resize(self.len(), false);
        self.fix_offset();
    }
    /// Keep the cursor inside the window like NC::Menu::scroll does.
    pub fn fix_offset(&mut self) {
        let h = self.height.max(1);
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
        self.offset = self.offset.min(self.len().saturating_sub(h));
    }
    pub fn mv(&mut self, d: isize) {
        self.mv_skip(d, |_| false);
    }
    /// Move, hopping over items that cannot take the cursor (separators, headers).
    pub fn mv_skip(&mut self, d: isize, skip: impl Fn(&T) -> bool) {
        if self.is_empty() {
            return;
        }
        let n = self.len() as isize;
        let mut c = (self.cursor as isize + d).clamp(0, n - 1);
        let step = if d < 0 { -1 } else { 1 };
        while skip(&self.items[c as usize]) {
            let next = c + step;
            if next < 0 || next >= n {
                // Nothing further that way: fall back to the nearest valid item.
                let mut back = c - step;
                while back >= 0 && back < n && skip(&self.items[back as usize]) {
                    back -= step;
                }
                if back < 0 || back >= n {
                    return;
                }
                c = back;
                break;
            }
            c = next;
        }
        self.cursor = c as usize;
        self.fix_offset();
    }
    pub fn page(&mut self, d: isize) {
        let h = self.height.max(1) as isize;
        self.offset =
            (self.offset as isize + d * h).clamp(0, (self.len() as isize - h).max(0)) as usize;
        self.mv(d * h);
    }
    pub fn home(&mut self) {
        self.cursor = 0;
        self.fix_offset();
    }
    pub fn toggle_sel(&mut self) {
        if let Some(s) = self.sel.get_mut(self.cursor) {
            *s = !*s;
        }
    }
    pub fn any_sel(&self) -> bool {
        self.sel.iter().any(|s| *s)
    }
    pub fn sel_idx(&self) -> Vec<usize> {
        self.sel
            .iter()
            .enumerate()
            .filter(|(_, s)| **s)
            .map(|(i, _)| i)
            .collect()
    }
    /// Selected items, or the one under the cursor when nothing is selected.
    pub fn chosen(&self) -> Vec<&T> {
        if self.any_sel() {
            self.sel_idx().into_iter().map(|i| &self.items[i]).collect()
        } else {
            self.cur().into_iter().collect()
        }
    }
    pub fn is_sel(&self, i: usize) -> bool {
        self.sel.get(i).copied().unwrap_or(false)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

pub enum SeItem {
    Field(usize),
    Sep,
    SearchIn,
    Mode,
    Search,
    Reset,
    Results,
    Song(Track),
    Artist(Ref),
    Album(Ref),
}

impl SeItem {
    pub fn inactive(&self) -> bool {
        matches!(self, SeItem::Sep | SeItem::Results)
    }
}

pub const SE_FIELDS: [&str; 11] = [
    "Any",
    "Artist",
    "Album Artist",
    "Title",
    "Album",
    "Filename",
    "Composer",
    "Performer",
    "Genre",
    "Date",
    "Comment",
];
const SE_MOPIDY: [&str; 11] = [
    "any",
    "artist",
    "albumartist",
    "track_name",
    "album",
    "uri",
    "composer",
    "performer",
    "genre",
    "date",
    "comment",
];
pub const SE_MODES: [&str; 2] = [
    "Match if tag contains searched phrase (no regexes)",
    "Match only if both values are the same",
];

#[derive(Clone)]
pub struct AlbumEntry {
    pub all: bool,
    pub r: Option<Ref>,
}

pub enum Purpose {
    Queue { play: bool, at: Option<usize> },
    AddPopup,
}

pub enum Msg {
    State(String),
    Current(Option<TlTrack>),
    Position(u64),
    Volume(i64),
    Queue(Vec<TlTrack>),
    Options(bool, bool, bool, bool),
    Browsed(String, Vec<Ref>),
    Tracks(Vec<Track>),
    Searched(Vec<Track>, Vec<Ref>, Vec<Ref>),
    Artists(Vec<Ref>),
    Albums(String, Vec<Ref>),
    AlbumTracks(String, Vec<String>),
    Playlists(Vec<Playlist>),
    PlaylistTracks(i64, Vec<String>),
    ReloadPlaylists,
    Resolved(Vec<String>, Purpose),
    LoggedIn(String),
    Me(i64),
    NeedLogin,
    Lyrics(String, Lyrics),
    Info(String),
    Error(String),
    Nothing,
}

pub enum Confirm {
    Clear,
    DeletePlaylist(i64),
}

pub enum PromptKind {
    Username,
    Password(String),
    Constraint(usize),
    Find(isize),
    Jump,
    SavePlaylist(Vec<String>),
    RenamePlaylist(i64),
    AddUri,
    Confirm(Confirm),
}

pub struct Prompt {
    pub label: String,
    pub buf: String,
    /// Byte offset of the edit cursor within `buf`
    pub cursor: usize,
    pub masked: bool,
    pub kind: PromptKind,
}

pub enum PopupKind {
    AddTo(Vec<String>),
    Where(Vec<String>),
}

pub struct Popup {
    pub title: String,
    pub items: Vec<String>,
    pub cursor: usize,
    pub kind: PopupKind,
}

pub enum LyricsState {
    Idle,
    Fetching(String),
    Loaded(String, Vec<(Option<u64>, String)>),
}

pub struct App {
    rpc: Arc<Rpc>,
    api: Arc<Api>,
    tx: mpsc::UnboundedSender<Msg>,
    pub token_path: PathBuf,
    pub lyrics_dir: PathBuf,
    creds: Option<(String, String)>,

    pub screen: Screen,
    prev_screen: Screen,
    pub quit: bool,
    pub connected: bool,
    pub mopidy_ok: bool,
    pub user_id: Option<i64>,
    pub last_input: Instant,

    pub state: PlayState,
    pub current: Option<TlTrack>,
    pos_ms: u64,
    pos_at: Instant,
    pub volume: i64,
    pub repeat: bool,
    pub random: bool,
    pub single: bool,
    pub consume: bool,
    /// Whether the playback modes have been loaded once (to stay quiet on startup)
    options_known: bool,
    ticks: u64,
    last_seek: Instant,
    seek_step: i64,

    pub queue: Menu<TlTrack>,
    pub queue_uris: HashSet<String>,
    pub queue_len_ms: u64,
    pub tracks: HashMap<String, Track>,

    pub browse_stack: Vec<(Option<String>, String, usize)>,
    pub browser: Menu<Ref>,
    browse_cache: HashMap<String, Vec<Ref>>,
    pub browse_loading: bool,

    pub se: Menu<SeItem>,
    pub se_fields: [String; 11],
    pub se_in_db: bool,
    pub se_mode: usize,
    pub se_searching: bool,
    pub se_tracks: Vec<Track>,
    pub se_artists: Vec<Ref>,
    pub se_albums: Vec<Ref>,
    /// Result tab like the web UI: 0 songs, 1 artists, 2 albums
    pub se_tab: usize,

    pub lib_artists: Menu<Ref>,
    pub lib_albums: Menu<AlbumEntry>,
    pub lib_songs: Menu<Track>,
    pub lib_col: usize,
    lib_albums_cache: HashMap<String, Vec<Ref>>,
    lib_tracks_cache: HashMap<String, Vec<String>>,
    lib_pending: HashSet<String>,
    pub lib_loading: bool,

    pub playlists: Menu<Playlist>,
    pub content: Menu<String>,
    pub ed_col: usize,
    pl_cache: HashMap<i64, Vec<String>>,

    pub lyrics: LyricsState,
    pub lyrics_offset: usize,
    pub lyrics_follow: bool,
    pub help_offset: usize,

    pub prompt: Option<Prompt>,
    pub popup: Option<Popup>,
    pub message: Option<(String, Instant)>,
    find: String,
    pub status_scroll: usize,
    pub title_scroll: usize,
}

const VOL_STEP: i64 = 1; // ncmpcpp volume_change_step = 1
const HIGHLIGHT_DELAY: Duration = Duration::from_secs(5);

impl App {
    pub fn new(
        rpc: Arc<Rpc>,
        api: Arc<Api>,
        tx: mpsc::UnboundedSender<Msg>,
        token_path: PathBuf,
        lyrics_dir: PathBuf,
        has_token: bool,
        creds: Option<(String, String)>,
    ) -> App {
        let mut se = Menu::default();
        se.set(Self::se_static());
        let mut app = App {
            rpc,
            api,
            tx,
            token_path,
            lyrics_dir,
            creds,
            screen: Screen::Playlist,
            prev_screen: Screen::Playlist,
            quit: false,
            connected: false,
            mopidy_ok: true,
            user_id: None,
            last_input: Instant::now(),
            state: PlayState::Stopped,
            current: None,
            pos_ms: 0,
            pos_at: Instant::now(),
            volume: 0,
            repeat: false,
            random: false,
            single: false,
            consume: false,
            options_known: false,
            ticks: 0,
            last_seek: Instant::now(),
            seek_step: 1,
            queue: Menu::default(),
            queue_uris: HashSet::new(),
            queue_len_ms: 0,
            tracks: HashMap::new(),
            browse_stack: vec![(None, String::new(), 0)],
            browser: Menu::default(),
            browse_cache: HashMap::new(),
            browse_loading: false,
            se,
            se_fields: Default::default(),
            se_in_db: true,
            se_mode: 0,
            se_searching: false,
            se_tracks: vec![],
            se_artists: vec![],
            se_albums: vec![],
            se_tab: 0,
            lib_artists: Menu::default(),
            lib_albums: Menu::default(),
            lib_songs: Menu::default(),
            lib_col: 0,
            lib_albums_cache: HashMap::new(),
            lib_tracks_cache: HashMap::new(),
            lib_pending: HashSet::new(),
            lib_loading: false,
            playlists: Menu::default(),
            content: Menu::default(),
            ed_col: 0,
            pl_cache: HashMap::new(),
            lyrics: LyricsState::Idle,
            lyrics_offset: 0,
            lyrics_follow: true,
            help_offset: 0,
            prompt: None,
            popup: None,
            message: None,
            find: String::new(),
            status_scroll: 0,
            title_scroll: 0,
        };
        if has_token {
            app.check_login();
        } else {
            app.need_login();
        }
        app
    }

    // ---------- helpers ----------

    fn task<F>(&self, f: F)
    where
        F: Future<Output = Result<Msg>> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let m = match f.await {
                Ok(m) => m,
                Err(e) => Msg::Error(e.to_string()),
            };
            let _ = tx.send(m);
        });
    }

    /// Statusbar::print — shown for message_delay_time (5 s).
    pub fn say(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), Instant::now()));
    }

    pub fn elapsed_ms(&self) -> u64 {
        match self.state {
            PlayState::Playing => self.pos_ms + self.pos_at.elapsed().as_millis() as u64,
            _ => self.pos_ms,
        }
    }

    fn set_pos(&mut self, ms: u64) {
        self.pos_ms = ms;
        self.pos_at = Instant::now();
    }

    pub fn track_of(&self, uri: &str) -> Option<&Track> {
        self.tracks.get(uri)
    }

    pub fn current_tlid(&self) -> Option<i64> {
        self.current.as_ref().map(|t| t.tlid)
    }

    /// playlist_disable_highlight_delay: the playlist cursor hides after 5 s idle.
    pub fn playlist_highlight(&self) -> bool {
        self.last_input.elapsed() < HIGHLIGHT_DELAY
    }

    fn remember(&mut self, tracks: impl IntoIterator<Item = Track>) {
        for t in tracks {
            self.tracks.insert(t.uri.clone(), t);
        }
    }

    /// Batch-lookup metadata for uris not in the cache (one RPC per 100 uris).
    fn lookup_missing(&self, uris: &[String]) {
        let missing: Vec<String> = uris
            .iter()
            .filter(|u| !self.tracks.contains_key(*u))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for chunk in missing.chunks(100) {
            let rpc = self.rpc.clone();
            let chunk = chunk.to_vec();
            self.task(async move {
                let v = rpc
                    .call("core.library.lookup", json!({"uris": chunk}))
                    .await?;
                Ok(Msg::Tracks(flatten_lookup(&v)))
            });
        }
    }

    /// Toggle a playback mode and announce the new state, like ncmpcpp's statusbar.
    fn toggle_mode(&mut self, method: &'static str, _name: &str, current: bool) {
        self.rpc_fire(method, json!({ "value": !current }));
    }

    fn rpc_fire(&self, method: &'static str, params: Value) {
        let rpc = self.rpc.clone();
        self.task(async move {
            rpc.call(method, params).await?;
            Ok(Msg::Nothing)
        });
    }

    // ---------- login ----------

    fn need_login(&mut self) {
        if let Some((u, p)) = self.creds.clone() {
            let api = self.api.clone();
            self.task(async move { Ok(Msg::LoggedIn(api.login(&u, &p).await?)) });
            return;
        }
        self.prompt = Some(Prompt {
            label: "Username: ".into(),
            buf: String::new(),
            cursor: 0,
            masked: false,
            kind: PromptKind::Username,
        });
    }

    fn check_login(&self) {
        let api = self.api.clone();
        self.task(async move {
            match api.me().await {
                Ok(id) => Ok(Msg::Me(id)),
                Err(e) if e.to_string().starts_with("unauthorized") => Ok(Msg::NeedLogin),
                Err(e) => Err(e),
            }
        });
    }

    // ---------- mopidy state ----------

    pub fn refresh_all(&mut self) {
        let rpc = self.rpc.clone();
        let tx = self.tx.clone();
        // Independent calls go out together: Mopidy serialises slow Tidal browses, so a
        // sequential chain would starve the queue behind the library.
        self.task(async move {
            let (s, c, p, v, q) = tokio::join!(
                rpc.call("core.playback.get_state", json!({})),
                rpc.call("core.playback.get_current_tl_track", json!({})),
                rpc.call("core.playback.get_time_position", json!({})),
                rpc.call("core.mixer.get_volume", json!({})),
                rpc.call("core.tracklist.get_tl_tracks", json!({})),
            );
            let _ = tx.send(Msg::State(s?.as_str().unwrap_or("stopped").into()));
            let _ = tx.send(Msg::Current(TlTrack::from_value(&c?)));
            let _ = tx.send(Msg::Position(p?.as_u64().unwrap_or(0)));
            let _ = tx.send(Msg::Volume(v?.as_i64().unwrap_or(0)));
            let _ = tx.send(Msg::Queue(TlTrack::list(&q?)));
            Ok(Msg::Nothing)
        });
        self.load_options();
        if self.user_id.is_some() {
            self.load_playlists();
        }
        if self.browser.is_empty() {
            self.browse_load();
        }
        if self.screen == Screen::Library && self.lib_artists.is_empty() {
            self.lib_load_artists();
        }
    }

    fn load_options(&self) {
        let rpc = self.rpc.clone();
        self.task(async move {
            let (r, z, y, c) = tokio::join!(
                rpc.call("core.tracklist.get_repeat", json!({})),
                rpc.call("core.tracklist.get_random", json!({})),
                rpc.call("core.tracklist.get_single", json!({})),
                rpc.call("core.tracklist.get_consume", json!({})),
            );
            let b = |v: Result<Value>| v.ok().and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(Msg::Options(b(r), b(z), b(y), b(c)))
        });
    }

    fn load_queue(&self) {
        let rpc = self.rpc.clone();
        self.task(async move {
            let q = rpc.call("core.tracklist.get_tl_tracks", json!({})).await?;
            Ok(Msg::Queue(TlTrack::list(&q)))
        });
    }

    fn sync_position(&self) {
        let rpc = self.rpc.clone();
        self.task(async move {
            let p = rpc
                .call("core.playback.get_time_position", json!({}))
                .await?;
            Ok(Msg::Position(p.as_u64().unwrap_or(0)))
        });
    }

    /// Resolve directories/albums/artists/playlists to track uris, then act (Purpose).
    fn resolve(&mut self, refs: Vec<Ref>, purpose: Purpose) {
        if refs.is_empty() {
            return;
        }
        if refs.iter().all(Ref::is_track) {
            let uris = refs.into_iter().map(|r| r.uri).collect();
            self.on_msg(Msg::Resolved(uris, purpose));
            return;
        }
        let rpc = self.rpc.clone();
        self.say("Adding...");
        self.task(async move {
            let mut uris = vec![];
            for r in refs {
                if r.is_track() {
                    uris.push(r.uri);
                } else {
                    uris.extend(expand_ref(&rpc, &r).await?);
                }
            }
            Ok(Msg::Resolved(uris, purpose))
        });
    }

    fn add_uris(&mut self, uris: Vec<String>, play: bool, at: Option<usize>) {
        if uris.is_empty() {
            self.say("No items to add");
            return;
        }
        let rpc = self.rpc.clone();
        let tx = self.tx.clone();
        let n = uris.len();
        let desc = if n == 1 {
            self.tracks.get(&uris[0]).map(|t| t.classic())
        } else {
            None
        };
        self.task(async move {
            let mut params = json!({"uris": uris});
            if let Some(p) = at {
                params["at_position"] = json!(p);
            }
            let added = rpc.call("core.tracklist.add", params).await?;
            let tl = TlTrack::list(&added);
            if play {
                if let Some(first) = tl.first() {
                    rpc.call("core.playback.play", json!({"tlid": first.tlid}))
                        .await?;
                }
            }
            if !play {
                let _ = tx.send(Msg::Info(match desc {
                    Some(d) => format!("Added to playlist: {d}"),
                    None => format!("Added {} items to playlist", tl.len()),
                }));
            }
            Ok(Msg::Nothing)
        });
    }

    // ---------- browser ----------

    fn browse_uri(&self) -> Option<String> {
        self.browse_stack.last().and_then(|(u, _, _)| u.clone())
    }

    fn browse_load(&mut self) {
        let uri = self.browse_uri();
        let key = uri.clone().unwrap_or_default();
        if let Some(items) = self.browse_cache.get(&key).cloned() {
            let mut items = items;
            if self.browse_stack.len() > 1 {
                items.insert(
                    0,
                    Ref {
                        kind: "parent".into(),
                        uri: String::new(),
                        name: "..".into(),
                    },
                );
            }
            let uris: Vec<String> = items
                .iter()
                .filter(|r| r.is_track())
                .map(|r| r.uri.clone())
                .collect();
            let cursor = self.browser.cursor;
            self.browser.set(items);
            self.browser.cursor = cursor;
            self.browser.clamp();
            self.lookup_missing(&uris);
            return;
        }
        self.browse_loading = true;
        let rpc = self.rpc.clone();
        self.task(async move {
            let v = rpc.call("core.library.browse", json!({"uri": uri})).await?;
            let refs: Vec<Ref> = serde_json::from_value(v)?;
            Ok(Msg::Browsed(key, refs))
        });
    }

    fn browse_into(&mut self, uri: String, name: String) {
        if let Some(top) = self.browse_stack.last_mut() {
            top.2 = self.browser.cursor;
        }
        self.browse_stack.push((Some(uri), name, 0));
        self.browser = Menu::default();
        self.browse_load();
    }

    fn browse_up(&mut self) {
        if self.browse_stack.len() > 1 {
            self.browse_stack.pop();
            let cursor = self.browse_stack.last().map(|t| t.2).unwrap_or(0);
            self.browser = Menu::default();
            self.browser.cursor = cursor;
            self.browse_load();
        }
    }

    pub fn browse_path(&self) -> String {
        let p: Vec<&str> = self
            .browse_stack
            .iter()
            .skip(1)
            .map(|(_, n, _)| n.as_str())
            .collect();
        if p.is_empty() {
            "/".into()
        } else {
            format!("/{}", p.join("/"))
        }
    }

    // ---------- search engine ----------

    fn se_static() -> Vec<SeItem> {
        let mut v: Vec<SeItem> = (0..SE_FIELDS.len()).map(SeItem::Field).collect();
        v.extend([
            SeItem::Sep,
            SeItem::SearchIn,
            SeItem::Mode,
            SeItem::Sep,
            SeItem::Search,
            SeItem::Reset,
        ]);
        v
    }

    fn se_reset(&mut self) {
        self.se_fields = Default::default();
        self.se_tracks.clear();
        self.se_artists.clear();
        self.se_albums.clear();
        self.se.set(Self::se_static());
    }

    /// Static form + results block for the active tab.
    fn se_rebuild(&mut self, keep_cursor: bool) {
        let mut items = Self::se_static();
        if self.se_has_results() {
            items.push(SeItem::Sep);
            items.push(SeItem::Results);
            items.push(SeItem::Sep);
            match self.se_tab {
                0 => items.extend(self.se_tracks.iter().cloned().map(SeItem::Song)),
                1 => items.extend(self.se_artists.iter().cloned().map(SeItem::Artist)),
                _ => items.extend(self.se_albums.iter().cloned().map(SeItem::Album)),
            }
        }
        let cursor = self.se.cursor;
        self.se.set(items);
        if self.se_has_results() {
            self.se.cursor = if keep_cursor && cursor >= 20 {
                cursor
            } else {
                20
            };
            self.se.clamp();
            if self.se.cursor < 20 {
                self.se.cursor = 20.min(self.se.len().saturating_sub(1));
            }
            self.se.fix_offset();
        }
    }

    pub fn se_has_results(&self) -> bool {
        !self.se_tracks.is_empty() || !self.se_artists.is_empty() || !self.se_albums.is_empty()
    }

    fn se_switch_tab(&mut self, d: isize) {
        self.se_tab = (self.se_tab as isize + d).rem_euclid(3) as usize;
        self.se_rebuild(false);
    }

    fn se_run(&mut self) {
        let mut query = serde_json::Map::new();
        for (i, f) in self.se_fields.iter().enumerate() {
            if !f.trim().is_empty() {
                query.insert(SE_MOPIDY[i].into(), json!([f.trim()]));
            }
        }
        if query.is_empty() {
            return;
        }
        self.se_searching = true;
        self.say("Searching...");
        self.se.set(Self::se_static());
        let exact = self.se_mode == 1;
        if !self.se_in_db {
            // Search in the current playlist: filter the queue locally.
            let fields = self.se_fields.clone();
            let hits: Vec<Track> = self
                .queue
                .items
                .iter()
                .map(|t| t.track.clone())
                .filter(|t| se_matches(t, &fields, exact))
                .collect();
            self.on_msg(Msg::Searched(hits, vec![], vec![]));
            return;
        }
        let rpc = self.rpc.clone();
        self.task(async move {
            let v = rpc
                .call(
                    "core.library.search",
                    json!({"query": query, "exact": exact}),
                )
                .await?;
            let mut seen = HashSet::new();
            let mut tracks = vec![];
            let mut artists = vec![];
            let mut albums = vec![];
            for res in v.as_array().into_iter().flatten() {
                for t in res
                    .get("tracks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(t) = Track::from_value(t) {
                        if seen.insert(t.uri.clone()) {
                            tracks.push(t);
                        }
                    }
                }
                for a in res
                    .get("artists")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let (Some(uri), Some(name)) = (
                        a.get("uri").and_then(Value::as_str),
                        a.get("name").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    if seen.insert(uri.to_string()) {
                        artists.push(Ref {
                            kind: "artist".into(),
                            uri: uri.into(),
                            name: name.into(),
                        });
                    }
                }
                for a in res
                    .get("albums")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let (Some(uri), Some(name)) = (
                        a.get("uri").and_then(Value::as_str),
                        a.get("name").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    if !seen.insert(uri.to_string()) {
                        continue;
                    }
                    let artist = a
                        .pointer("/artists/0/name")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let label = if artist.is_empty() {
                        name.to_string()
                    } else {
                        format!("{artist} - {name}")
                    };
                    albums.push(Ref {
                        kind: "album".into(),
                        uri: uri.into(),
                        name: label,
                    });
                }
            }
            Ok(Msg::Searched(tracks, artists, albums))
        });
    }

    // ---------- media library ----------

    fn lib_load_artists(&mut self) {
        self.lib_loading = true;
        let rpc = self.rpc.clone();
        self.task(async move {
            let mut artists: Vec<Ref> = vec![];
            for root in ["local:directory?type=artist", "tidal:my_artists"] {
                if let Ok(v) = rpc.call("core.library.browse", json!({"uri": root})).await {
                    if let Ok(refs) = serde_json::from_value::<Vec<Ref>>(v) {
                        artists.extend(refs.into_iter().filter(|r| !r.name.trim().is_empty()).map(
                            |mut r| {
                                r.name = r.name.trim().to_string();
                                r
                            },
                        ));
                    }
                }
            }
            artists.sort_by_key(|r| r.name.to_lowercase());
            Ok(Msg::Artists(artists))
        });
    }

    /// Albums column follows the artist cursor (cached per artist).
    fn lib_sync_albums(&mut self) {
        let Some(artist) = self.lib_artists.cur().cloned() else {
            self.lib_albums.set(vec![]);
            self.lib_songs.set(vec![]);
            return;
        };
        if let Some(albums) = self.lib_albums_cache.get(&artist.uri).cloned() {
            let mut entries = vec![AlbumEntry { all: true, r: None }];
            entries.extend(albums.into_iter().map(|r| AlbumEntry {
                all: false,
                r: Some(r),
            }));
            self.lib_albums.set(entries);
            self.lib_albums.home();
            self.lib_sync_songs();
            return;
        }
        if self.lib_pending.insert(artist.uri.clone()) {
            let rpc = self.rpc.clone();
            self.task(async move {
                let v = rpc
                    .call("core.library.browse", json!({"uri": artist.uri}))
                    .await?;
                let refs: Vec<Ref> = serde_json::from_value(v)?;
                // Tidal lists an artist's albums as "album" refs; Mopidy-Local lists them as
                // "directory" refs (local:directory?type=track&album=...).
                let mut albums: Vec<Ref> =
                    refs.iter().filter(|r| r.kind == "album").cloned().collect();
                if albums.is_empty() {
                    albums = refs.into_iter().filter(|r| r.kind == "directory").collect();
                }
                albums.dedup_by(|a, b| a.uri == b.uri);
                Ok(Msg::Albums(artist.uri, albums))
            });
        }
    }

    /// Songs column follows the album cursor; "All tracks" joins every album.
    fn lib_sync_songs(&mut self) {
        let Some(entry) = self.lib_albums.cur().cloned() else {
            self.lib_songs.set(vec![]);
            return;
        };
        let album_uris: Vec<String> = if entry.all {
            self.lib_albums
                .items
                .iter()
                .filter_map(|e| e.r.as_ref().map(|r| r.uri.clone()))
                .collect()
        } else {
            entry.r.iter().map(|r| r.uri.clone()).collect()
        };
        let mut songs = vec![];
        let mut missing = vec![];
        let mut to_fetch = vec![];
        for a in &album_uris {
            match self.lib_tracks_cache.get(a) {
                Some(uris) => {
                    for u in uris {
                        if let Some(t) = self.tracks.get(u) {
                            songs.push(t.clone());
                        } else {
                            missing.push(u.clone());
                        }
                    }
                }
                None => {
                    if self.lib_pending.insert(a.clone()) {
                        to_fetch.push(a.clone());
                    }
                }
            }
        }
        if !to_fetch.is_empty() {
            // One album at a time: Mopidy runs Tidal browses serially anyway, and a burst
            // would delay everything else the user does meanwhile.
            let rpc = self.rpc.clone();
            let tx = self.tx.clone();
            self.task(async move {
                for a in to_fetch {
                    let v = rpc.call("core.library.browse", json!({"uri": a})).await?;
                    let refs: Vec<Ref> = serde_json::from_value(v)?;
                    let _ = tx.send(Msg::AlbumTracks(
                        a,
                        refs.into_iter()
                            .filter(Ref::is_track)
                            .map(|r| r.uri)
                            .collect(),
                    ));
                }
                Ok(Msg::Nothing)
            });
        }
        self.lookup_missing(&missing);
        let cursor = self.lib_songs.cursor;
        self.lib_songs.set(songs);
        self.lib_songs.cursor = cursor;
        self.lib_songs.clamp();
    }

    fn lib_chosen_refs(&self) -> Vec<Ref> {
        match self.lib_col {
            0 => self.lib_artists.chosen().into_iter().cloned().collect(),
            1 => {
                let mut out = vec![];
                for e in self.lib_albums.chosen() {
                    match &e.r {
                        Some(r) => out.push(r.clone()),
                        None => {
                            out.extend(self.lib_albums.items.iter().filter_map(|e| e.r.clone()))
                        }
                    }
                }
                out
            }
            _ => self.lib_songs.chosen().into_iter().map(track_ref).collect(),
        }
    }

    // ---------- playlist editor (Gleam backend) ----------

    fn load_playlists(&self) {
        let api = self.api.clone();
        self.task(async move { Ok(Msg::Playlists(api.playlists().await?)) });
    }

    fn owns(&self, p: &Playlist) -> bool {
        self.user_id == Some(p.user_id)
    }

    fn ed_sync_content(&mut self) {
        let Some(p) = self.playlists.cur().cloned() else {
            self.content.set(vec![]);
            return;
        };
        if let Some(t) = self.pl_cache.get(&p.id).cloned() {
            self.lookup_missing(&t);
            let cursor = self.content.cursor;
            self.content.set(t);
            self.content.cursor = cursor;
            self.content.clamp();
            return;
        }
        let api = self.api.clone();
        self.task(async move { Ok(Msg::PlaylistTracks(p.id, api.playlist_tracks(p.id).await?)) });
    }

    fn add_to_playlist(&mut self, pid: i64, name: String, uris: Vec<String>) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        self.pl_cache.remove(&pid);
        self.say("Adding...");
        self.task(async move {
            for u in uris {
                api.add_track(pid, &u).await?;
            }
            let _ = tx.send(Msg::ReloadPlaylists);
            Ok(Msg::Info(format!(
                "Selected item(s) added to playlist \"{name}\""
            )))
        });
    }

    fn save_playlist(&mut self, name: String, uris: Vec<String>) {
        if uris.is_empty() {
            return;
        }
        let api = self.api.clone();
        let tx = self.tx.clone();
        self.say("Saving...");
        self.task(async move {
            let p = api.create_playlist(&name).await?;
            for u in uris {
                api.add_track(p.id, &u).await?;
            }
            let _ = tx.send(Msg::ReloadPlaylists);
            Ok(Msg::Info(format!("Playlist saved as \"{name}\"")))
        });
    }

    /// `a`: NC "Add selected item(s) to..." dialog.
    fn open_add_popup(&mut self, uris: Vec<String>) {
        if uris.is_empty() {
            return self.say("No selected songs");
        }
        let mut items = vec!["Current playlist".to_string()];
        items.extend(
            self.playlists
                .items
                .iter()
                .filter(|p| self.owns(p))
                .map(|p| p.name.clone()),
        );
        items.push("New playlist".into());
        items.push("Cancel".into());
        self.popup = Some(Popup {
            title: "Add selected item(s) to...".into(),
            items,
            cursor: 0,
            kind: PopupKind::AddTo(uris),
        });
    }

    // ---------- lyrics ----------

    fn ensure_lyrics(&mut self) {
        let Some(cur) = self.current.as_ref().map(|c| c.track.clone()) else {
            return;
        };
        match &self.lyrics {
            LyricsState::Loaded(u, _) | LyricsState::Fetching(u) if *u == cur.uri => return,
            _ => {}
        }
        self.lyrics = LyricsState::Fetching(cur.uri.clone());
        self.lyrics_offset = 0;
        self.lyrics_follow = true;
        let api = self.api.clone();
        let dir = self.lyrics_dir.clone();
        self.task(async move {
            let uri = cur.uri.clone();
            let l = api.lyrics(&cur, &dir).await?;
            Ok(Msg::Lyrics(uri, l))
        });
    }

    /// Index of the lyric line active at the current position (synced lyrics only).
    pub fn lyrics_active(&self) -> Option<usize> {
        let LyricsState::Loaded(_, lines) = &self.lyrics else {
            return None;
        };
        let now = self.elapsed_ms();
        let mut active = None;
        for (i, (ts, _)) in lines.iter().enumerate() {
            match ts {
                Some(t) if *t <= now => active = Some(i),
                Some(_) => break,
                None => return None,
            }
        }
        active
    }

    // ---------- messages from tasks / mopidy ----------

    pub fn tick(&mut self) {
        self.ticks += 1;
        self.status_scroll += 1;
        self.title_scroll += 1;
        if self.state == PlayState::Playing && self.ticks.is_multiple_of(10) && self.connected {
            self.sync_position();
        }
        if let Some((_, at)) = &self.message {
            if at.elapsed().as_secs() >= 5 {
                self.message = None;
            }
        }
        if self.screen == Screen::Lyrics {
            self.ensure_lyrics();
        }
    }

    pub fn on_connected(&mut self) {
        self.connected = true;
        self.refresh_all();
    }

    pub fn on_disconnected(&mut self) {
        self.connected = false;
    }

    pub fn on_event(&mut self, v: Value) {
        if let Some(err) = v.get("error").and_then(Value::as_str) {
            self.say(format!("Backend: {err}"));
            return;
        }
        let ev = v.get("event").and_then(Value::as_str).unwrap_or("");
        let tl = v.get("tl_track").and_then(TlTrack::from_value);
        let tp = v.get("time_position").and_then(Value::as_u64);
        match ev {
            "track_playback_started" => {
                self.state = PlayState::Playing;
                if let Some(t) = &tl {
                    self.tracks.insert(t.track.uri.clone(), t.track.clone());
                }
                self.current = tl;
                self.set_pos(0);
                self.status_scroll = 0;
            }
            "track_playback_paused" => {
                self.state = PlayState::Paused;
                if tl.is_some() {
                    self.current = tl;
                }
                self.set_pos(tp.unwrap_or(self.pos_ms));
            }
            "track_playback_resumed" => {
                self.state = PlayState::Playing;
                if tl.is_some() {
                    self.current = tl;
                }
                self.set_pos(tp.unwrap_or(self.pos_ms));
            }
            "playback_state_changed" => {
                match v.get("new_state").and_then(Value::as_str) {
                    Some("playing") => self.state = PlayState::Playing,
                    Some("paused") => self.state = PlayState::Paused,
                    _ => {
                        self.state = PlayState::Stopped;
                        self.current = None;
                        self.set_pos(0);
                    }
                }
                let m = self.pos_ms;
                self.set_pos(m);
            }
            "tracklist_changed" => self.load_queue(),
            "volume_changed" => {
                self.volume = v
                    .get("volume")
                    .and_then(Value::as_i64)
                    .unwrap_or(self.volume)
            }
            "seeked" => self.set_pos(tp.unwrap_or(0)),
            "options_changed" => self.load_options(),
            "mopidy_connected" => {
                self.mopidy_ok = true;
                self.refresh_all();
            }
            "mopidy_disconnected" => {
                self.mopidy_ok = false;
                self.say("Connection to Mopidy lost");
            }
            _ => {}
        }
    }

    pub fn on_msg(&mut self, m: Msg) {
        match m {
            Msg::Nothing => {}
            Msg::Info(s) => self.say(s),
            Msg::Error(e) => {
                if e == "connection lost" {
                    // a reconnect refreshes everything by itself
                } else if e.starts_with("unauthorized") && self.user_id.is_none() {
                    self.say("Not logged in (L)")
                } else {
                    self.say(e)
                }
            }
            Msg::State(s) => {
                self.state = match s.as_str() {
                    "playing" => PlayState::Playing,
                    "paused" => PlayState::Paused,
                    _ => PlayState::Stopped,
                };
                let m = self.pos_ms;
                self.set_pos(m);
            }
            Msg::Current(c) => {
                if let Some(t) = &c {
                    self.tracks.insert(t.track.uri.clone(), t.track.clone());
                }
                self.current = c;
            }
            Msg::Position(p) => self.set_pos(p),
            Msg::Volume(v) => self.volume = v,
            Msg::Queue(q) => {
                self.remember(q.iter().map(|t| t.track.clone()));
                self.queue_uris = q.iter().map(|t| t.track.uri.clone()).collect();
                self.queue_len_ms = q.iter().filter_map(|t| t.track.length).sum();
                // Keep selection by tlid across reloads (moves, removals).
                let selected: HashSet<i64> = self
                    .queue
                    .sel_idx()
                    .iter()
                    .map(|i| self.queue.items[*i].tlid)
                    .collect();
                let cursor_tlid = self.queue.cur().map(|t| t.tlid);
                self.queue.set(q);
                for (i, t) in self.queue.items.iter().enumerate() {
                    self.queue.sel[i] = selected.contains(&t.tlid);
                }
                if let Some(c) = cursor_tlid {
                    if let Some(i) = self.queue.items.iter().position(|t| t.tlid == c) {
                        self.queue.cursor = i;
                    }
                }
                self.queue.clamp();
                self.title_scroll = 0;
            }
            Msg::Options(r, z, y, c) => {
                // Announce each mode that changed, whoever changed it (us or another
                // client), like ncmpcpp. Silent on the first load so startup is quiet.
                if self.options_known {
                    for (old, new, name) in [
                        (self.repeat, r, "Repeat"),
                        (self.random, z, "Random"),
                        (self.single, y, "Single"),
                        (self.consume, c, "Consume"),
                    ] {
                        if old != new {
                            self.say(format!("{name} mode is {}", if new { "on" } else { "off" }));
                        }
                    }
                }
                self.options_known = true;
                self.repeat = r;
                self.random = z;
                self.single = y;
                self.consume = c;
            }
            Msg::Browsed(uri, refs) => {
                self.browse_loading = false;
                self.browse_cache.insert(uri.clone(), refs);
                if self.browse_uri().unwrap_or_default() == uri {
                    self.browse_load();
                }
            }
            Msg::Tracks(ts) => {
                self.remember(ts);
                if self.screen == Screen::Library {
                    self.lib_sync_songs();
                }
            }
            Msg::Searched(tracks, artists, albums) => {
                self.se_searching = false;
                self.remember(tracks.iter().cloned());
                self.se_tracks = tracks;
                self.se_artists = artists;
                self.se_albums = albums;
                // Songs first; fall back to whichever tab has something
                self.se_tab = if !self.se_tracks.is_empty() {
                    0
                } else if !self.se_artists.is_empty() {
                    1
                } else {
                    2
                };
                if self.se_has_results() {
                    self.say("Searching finished");
                } else {
                    self.say("No results found");
                }
                self.se_rebuild(false);
            }
            Msg::Artists(a) => {
                self.lib_loading = false;
                self.lib_artists.set(a);
                self.lib_sync_albums();
            }
            Msg::Albums(artist, albums) => {
                self.lib_pending.remove(&artist);
                self.lib_albums_cache.insert(artist.clone(), albums);
                if self.lib_artists.cur().map(|a| a.uri.as_str()) == Some(artist.as_str()) {
                    self.lib_sync_albums();
                }
            }
            Msg::AlbumTracks(album, uris) => {
                self.lib_pending.remove(&album);
                self.lib_tracks_cache.insert(album, uris);
                self.lib_sync_songs();
            }
            Msg::Playlists(p) => {
                self.playlists.set(p);
                self.ed_sync_content();
            }
            Msg::PlaylistTracks(id, uris) => {
                self.pl_cache.insert(id, uris);
                if self.playlists.cur().map(|p| p.id) == Some(id) {
                    self.ed_sync_content();
                }
            }
            Msg::ReloadPlaylists => {
                self.pl_cache.clear();
                self.load_playlists();
            }
            Msg::Resolved(uris, purpose) => match purpose {
                Purpose::Queue { play, at } => self.add_uris(uris, play, at),
                Purpose::AddPopup => self.open_add_popup(uris),
            },
            Msg::NeedLogin => {
                let _ = std::fs::remove_file(&self.token_path);
                self.api.set_token(None);
                self.need_login();
            }
            Msg::Me(id) => {
                self.user_id = Some(id);
                self.load_playlists();
            }
            Msg::LoggedIn(token) => {
                if let Some(parent) = self.token_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&self.token_path, &token);
                self.api.set_token(Some(token.clone()));
                self.rpc.set_token(Some(token));
                self.check_login();
            }
            Msg::Lyrics(uri, l) => {
                if matches!(&self.lyrics, LyricsState::Fetching(u) if *u == uri) {
                    self.lyrics = LyricsState::Loaded(uri, l.lines());
                    self.lyrics_offset = 0;
                }
            }
        }
    }

    // ---------- keys ----------

    pub fn key(&mut self, k: KeyEvent) {
        self.last_input = Instant::now();
        if self.prompt.is_some() {
            return self.prompt_key(k);
        }
        if self.popup.is_some() {
            return self.popup_key(k);
        }
        let columns = matches!(self.screen, Screen::Library | Screen::Editor)
            || (self.screen == Screen::Search && self.se_has_results());
        match k.code {
            K::Char('q') => self.quit = true,
            K::F(1) => self.goto(Screen::Help),
            K::Char('1') => self.goto(Screen::Playlist),
            K::Char('2') => self.goto(Screen::Browser),
            K::Char('3') => self.goto(Screen::Search),
            K::Char('4') => self.goto(Screen::Library),
            K::Char('5') => self.goto(Screen::Editor),
            K::Char('l') => {
                if self.screen == Screen::Lyrics {
                    self.goto(self.prev_screen);
                } else {
                    self.goto(Screen::Lyrics);
                }
            }
            K::Tab | K::BackTab => {
                // screen_switcher_mode = playlist, browser
                self.goto(if self.screen == Screen::Playlist {
                    Screen::Browser
                } else {
                    Screen::Playlist
                });
            }
            K::Char('s') => self.rpc_fire("core.playback.stop", json!({})),
            K::Char('p') | K::Char('P') => match self.state {
                PlayState::Playing => self.rpc_fire("core.playback.pause", json!({})),
                PlayState::Paused => self.rpc_fire("core.playback.resume", json!({})),
                PlayState::Stopped => self.rpc_fire("core.playback.play", json!({})),
            },
            K::Char('>') => self.rpc_fire("core.playback.next", json!({})),
            K::Char('<') => self.rpc_fire("core.playback.previous", json!({})),
            K::Char('f') => self.seek(1),
            K::Char('b') => self.seek(-1),
            K::Right if columns => self.next_column(1),
            K::Left if columns => self.next_column(-1),
            K::Right | K::Char('+') => self.set_volume(self.volume + VOL_STEP),
            K::Left | K::Char('-') => self.set_volume(self.volume - VOL_STEP),
            K::Char('r') => self.toggle_mode("core.tracklist.set_repeat", "Repeat", self.repeat),
            K::Char('z') => self.toggle_mode("core.tracklist.set_random", "Random", self.random),
            K::Char('y') => self.toggle_mode("core.tracklist.set_single", "Single", self.single),
            K::Char('R') => self.toggle_mode("core.tracklist.set_consume", "Consume", self.consume),
            K::Char('Z') => {
                self.rpc_fire("core.tracklist.shuffle", json!({}));
                self.say("Playlist shuffled");
            }
            K::Char('c') => self.ask(
                PromptKind::Confirm(Confirm::Clear),
                "Do you really want to clear the playlist? [y/n] ",
            ),
            K::Char('C') => {
                let keep: HashSet<i64> = if self.queue.any_sel() {
                    self.queue
                        .sel_idx()
                        .iter()
                        .map(|i| self.queue.items[*i].tlid)
                        .collect()
                } else {
                    self.current_tlid().into_iter().collect()
                };
                let rm: Vec<i64> = self
                    .queue
                    .items
                    .iter()
                    .map(|t| t.tlid)
                    .filter(|t| !keep.contains(t))
                    .collect();
                if !rm.is_empty() {
                    self.rpc_fire("core.tracklist.remove", json!({"criteria": {"tlid": rm}}));
                }
            }
            K::Char('u') => {
                self.rpc_fire("core.library.refresh", json!({}));
                self.browse_cache.clear();
                self.say("Database update started");
            }
            K::Char('/') => self.ask(PromptKind::Find(1), "Find: "),
            K::Char('?') => self.ask(PromptKind::Find(-1), "Find: "),
            K::Char('.') => self.find_next(1),
            K::Char(',') => self.find_next(-1),
            K::Char('g') => self.ask(
                PromptKind::Jump,
                "Position to go (in %/h:m:ss/m:ss/seconds(s)): ",
            ),
            K::Char('o') => {
                if let Some(cur) = self.current_tlid() {
                    if let Some(i) = self.queue.items.iter().position(|t| t.tlid == cur) {
                        self.queue.cursor = i;
                        self.queue.fix_offset();
                    }
                    if self.screen == Screen::Lyrics {
                        self.lyrics_follow = true;
                    } else {
                        self.goto(Screen::Playlist);
                    }
                }
            }
            K::Char('S') => {
                let uris: Vec<String> = self
                    .queue
                    .items
                    .iter()
                    .map(|t| t.track.uri.clone())
                    .collect();
                if uris.is_empty() {
                    self.say("Playlist is empty");
                } else {
                    self.ask(PromptKind::SavePlaylist(uris), "Save playlist as: ");
                }
            }
            K::Char('A') => self.ask(PromptKind::AddUri, "Add: "),
            K::Char('a') => {
                let refs = self.chosen_refs();
                self.resolve(refs, Purpose::AddPopup);
            }
            K::Char('v') => self.with_menu_sel(|m| {
                for s in m.sel.iter_mut() {
                    *s = !*s;
                }
            }),
            K::Char('V') => self.with_menu_sel(|m| m.sel.iter_mut().for_each(|s| *s = false)),
            K::Char('L') => {
                let _ = std::fs::remove_file(&self.token_path);
                self.api.set_token(None);
                self.rpc.set_token(None);
                self.user_id = None;
                self.creds = None;
                self.prompt = Some(Prompt {
                    label: "Username: ".into(),
                    buf: String::new(),
                    cursor: 0,
                    masked: false,
                    kind: PromptKind::Username,
                });
            }
            K::Char('`') if self.screen == Screen::Lyrics => {
                if let Some(c) = &self.current {
                    let _ = std::fs::remove_file(
                        self.lyrics_dir
                            .join(format!("{}.json", lyrics_key(&c.track.uri))),
                    );
                }
                self.lyrics = LyricsState::Idle;
                self.ensure_lyrics();
            }
            _ => self.screen_key(k),
        }
    }

    fn goto(&mut self, s: Screen) {
        if s != self.screen {
            self.prev_screen = self.screen;
            self.screen = s;
            self.title_scroll = 0;
        }
        if s == Screen::Lyrics {
            self.lyrics_follow = true;
            self.ensure_lyrics();
        }
        if s == Screen::Library
            && self.lib_artists.is_empty()
            && !self.lib_loading
            && self.connected
        {
            self.lib_load_artists();
        }
    }

    fn ask(&mut self, kind: PromptKind, label: &str) {
        let masked = matches!(kind, PromptKind::Password(_));
        let buf = match &kind {
            PromptKind::Constraint(i) => self.se_fields[*i].clone(),
            PromptKind::RenamePlaylist(id) => self
                .playlists
                .items
                .iter()
                .find(|p| p.id == *id)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let cursor = buf.len();
        self.prompt = Some(Prompt {
            label: label.into(),
            buf,
            cursor,
            masked,
            kind,
        });
    }

    /// seek_time = 1 s, incremental_seeking: repeated presses step further.
    fn seek(&mut self, dir: i64) {
        let Some(len) = self.current.as_ref().and_then(|c| c.track.length) else {
            return;
        };
        if self.last_seek.elapsed() < Duration::from_millis(400) {
            self.seek_step = (self.seek_step + 1).min(30);
        } else {
            self.seek_step = 1;
        }
        self.last_seek = Instant::now();
        let pos = (self.elapsed_ms() as i64 + dir * self.seek_step * 1000).clamp(0, len as i64 - 1)
            as u64;
        self.set_pos(pos);
        self.rpc_fire("core.playback.seek", json!({"time_position": pos}));
    }

    fn set_volume(&mut self, v: i64) {
        let v = v.clamp(0, 100);
        self.volume = v;
        self.rpc_fire("core.mixer.set_volume", json!({"volume": v}));
    }

    fn next_column(&mut self, d: isize) {
        match self.screen {
            Screen::Library => {
                self.lib_col = (self.lib_col as isize + d).clamp(0, 2) as usize;
            }
            Screen::Editor => {
                self.ed_col = (self.ed_col as isize + d).clamp(0, 1) as usize;
            }
            Screen::Search => self.se_switch_tab(d),
            _ => {}
        }
    }

    /// Items under the cursor/selection on the current screen, as refs to resolve.
    fn chosen_refs(&self) -> Vec<Ref> {
        match self.screen {
            Screen::Playlist => self
                .queue
                .chosen()
                .into_iter()
                .map(|t| track_ref(&t.track))
                .collect(),
            Screen::Browser => self
                .browser
                .chosen()
                .into_iter()
                .filter(|r| r.kind != "parent")
                .cloned()
                .collect(),
            Screen::Search => self
                .se
                .chosen()
                .into_iter()
                .filter_map(|i| match i {
                    SeItem::Song(t) => Some(track_ref(t)),
                    SeItem::Artist(r) | SeItem::Album(r) => Some(r.clone()),
                    _ => None,
                })
                .collect(),
            Screen::Library => self.lib_chosen_refs(),
            Screen::Editor => {
                if self.ed_col == 0 {
                    self.content.items.iter().map(|u| uri_ref(u)).collect()
                } else {
                    self.content
                        .chosen()
                        .into_iter()
                        .map(|u| uri_ref(u))
                        .collect()
                }
            }
            Screen::Lyrics | Screen::Help => vec![],
        }
    }

    fn with_menu_sel(&mut self, f: impl FnOnce(&mut MenuSel)) {
        let mut m = match self.screen {
            Screen::Playlist => MenuSel {
                sel: &mut self.queue.sel,
            },
            Screen::Browser => MenuSel {
                sel: &mut self.browser.sel,
            },
            Screen::Search => MenuSel {
                sel: &mut self.se.sel,
            },
            Screen::Library => match self.lib_col {
                0 => MenuSel {
                    sel: &mut self.lib_artists.sel,
                },
                1 => MenuSel {
                    sel: &mut self.lib_albums.sel,
                },
                _ => MenuSel {
                    sel: &mut self.lib_songs.sel,
                },
            },
            Screen::Editor => MenuSel {
                sel: &mut self.content.sel,
            },
            _ => return,
        };
        f(&mut m);
    }

    /// Row text for `/` find on the current screen.
    fn row_text(&self, i: usize) -> Option<String> {
        Some(match self.screen {
            Screen::Playlist => self.queue.items.get(i)?.track.classic(),
            Screen::Browser => {
                let r = self.browser.items.get(i)?;
                if r.is_track() {
                    self.track_of(&r.uri)
                        .map(|t| t.classic())
                        .unwrap_or(r.name.clone())
                } else {
                    r.name.clone()
                }
            }
            Screen::Search => match self.se.items.get(i)? {
                SeItem::Song(t) => t.classic(),
                SeItem::Artist(r) | SeItem::Album(r) => r.name.clone(),
                _ => return None,
            },
            Screen::Library => match self.lib_col {
                0 => self.lib_artists.items.get(i)?.name.clone(),
                1 => self
                    .lib_albums
                    .items
                    .get(i)?
                    .r
                    .as_ref()
                    .map(|r| r.name.clone())
                    .unwrap_or("All tracks".into()),
                _ => self.lib_songs.items.get(i)?.library(),
            },
            Screen::Editor => {
                if self.ed_col == 0 {
                    self.playlists.items.get(i)?.name.clone()
                } else {
                    let u = self.content.items.get(i)?;
                    self.track_of(u).map(|t| t.classic()).unwrap_or(u.clone())
                }
            }
            _ => return None,
        })
    }

    fn active_len_cursor(&self) -> (usize, usize) {
        match self.screen {
            Screen::Playlist => (self.queue.len(), self.queue.cursor),
            Screen::Browser => (self.browser.len(), self.browser.cursor),
            Screen::Search => (self.se.len(), self.se.cursor),
            Screen::Library => match self.lib_col {
                0 => (self.lib_artists.len(), self.lib_artists.cursor),
                1 => (self.lib_albums.len(), self.lib_albums.cursor),
                _ => (self.lib_songs.len(), self.lib_songs.cursor),
            },
            Screen::Editor => {
                if self.ed_col == 0 {
                    (self.playlists.len(), self.playlists.cursor)
                } else {
                    (self.content.len(), self.content.cursor)
                }
            }
            _ => (0, 0),
        }
    }

    fn set_cursor(&mut self, i: usize) {
        match self.screen {
            Screen::Playlist => self.queue.cursor = i,
            Screen::Browser => self.browser.cursor = i,
            Screen::Search => self.se.cursor = i,
            Screen::Library => match self.lib_col {
                0 => self.lib_artists.cursor = i,
                1 => self.lib_albums.cursor = i,
                _ => self.lib_songs.cursor = i,
            },
            Screen::Editor => {
                if self.ed_col == 0 {
                    self.playlists.cursor = i
                } else {
                    self.content.cursor = i
                }
            }
            _ => {}
        }
        self.after_move();
    }

    fn find_next(&mut self, dir: isize) {
        if self.find.is_empty() {
            return;
        }
        let needle = self.find.to_lowercase();
        let (n, start) = self.active_len_cursor();
        if n == 0 {
            return;
        }
        for step in 1..=n {
            let i = (start as isize + dir * step as isize).rem_euclid(n as isize) as usize;
            if self
                .row_text(i)
                .is_some_and(|t| t.to_lowercase().contains(&needle))
            {
                return self.set_cursor(i);
            }
        }
        self.say(format!("Unable to find \"{}\"", self.find));
    }

    fn after_move(&mut self) {
        match self.screen {
            Screen::Playlist => self.queue.fix_offset(),
            Screen::Browser => self.browser.fix_offset(),
            Screen::Search => self.se.fix_offset(),
            Screen::Library => match self.lib_col {
                0 => {
                    self.lib_artists.fix_offset();
                    self.lib_sync_albums();
                }
                1 => {
                    self.lib_albums.fix_offset();
                    self.lib_sync_songs();
                }
                _ => self.lib_songs.fix_offset(),
            },
            Screen::Editor => {
                if self.ed_col == 0 {
                    self.playlists.fix_offset();
                    self.content.cursor = 0;
                    self.ed_sync_content();
                } else {
                    self.content.fix_offset();
                }
            }
            Screen::Lyrics => self.lyrics_follow = false,
            Screen::Help => {}
        }
    }

    /// Movement keys shared by every list screen.
    fn mv(&mut self, d: isize, page: bool) {
        match self.screen {
            Screen::Lyrics => {
                let n = match &self.lyrics {
                    LyricsState::Loaded(_, l) => l.len(),
                    _ => 0,
                };
                let step = if page { 10 } else { 1 };
                self.lyrics_offset = (self.lyrics_offset as isize + d * step)
                    .clamp(0, n.saturating_sub(1) as isize)
                    as usize;
                self.lyrics_follow = false;
                return;
            }
            Screen::Help => {
                let step = if page { 10 } else { 1 };
                self.help_offset = (self.help_offset as isize + d * step)
                    .clamp(0, crate::ui::help_len() as isize - 1)
                    as usize;
                return;
            }
            _ => {}
        }
        macro_rules! go {
            ($m:expr) => {
                if page {
                    $m.page(d)
                } else {
                    $m.mv(d)
                }
            };
        }
        match self.screen {
            Screen::Playlist => go!(self.queue),
            Screen::Browser => go!(self.browser),
            Screen::Search => {
                if page {
                    self.se.page(d);
                } else {
                    self.se.mv_skip(d, SeItem::inactive);
                }
            }
            Screen::Library => match self.lib_col {
                0 => go!(self.lib_artists),
                1 => go!(self.lib_albums),
                _ => go!(self.lib_songs),
            },
            Screen::Editor => {
                if self.ed_col == 0 {
                    go!(self.playlists)
                } else {
                    go!(self.content)
                }
            }
            _ => {}
        }
        self.after_move();
    }

    fn screen_key(&mut self, k: KeyEvent) {
        match k.code {
            K::Down | K::Char('j') => return self.mv(1, false),
            K::Up | K::Char('k') => return self.mv(-1, false),
            K::PageDown => return self.mv(1, true),
            K::PageUp => return self.mv(-1, true),
            K::Home => {
                let (n, _) = self.active_len_cursor();
                if n > 0 {
                    return self.set_cursor(0);
                }
                return;
            }
            K::End => {
                let (n, _) = self.active_len_cursor();
                if n > 0 {
                    return self.set_cursor(n - 1);
                }
                return;
            }
            _ => {}
        }
        match self.screen {
            Screen::Playlist => self.playlist_key(k),
            Screen::Browser => self.browser_key(k),
            Screen::Search => self.search_key(k),
            Screen::Library => self.library_key(k),
            Screen::Editor => self.editor_key(k),
            Screen::Lyrics | Screen::Help => {}
        }
    }

    fn playlist_key(&mut self, k: KeyEvent) {
        let i = self.queue.cursor;
        let Some(t) = self.queue.cur().cloned() else {
            return;
        };
        match k.code {
            K::Enter => self.rpc_fire("core.playback.play", json!({"tlid": t.tlid})),
            K::Char(' ') => {
                self.queue.toggle_sel();
                self.queue.mv(1);
            }
            K::Backspace => {
                self.set_pos(0);
                self.rpc_fire("core.playback.seek", json!({"time_position": 0}));
            }
            K::Char('d') | K::Delete => {
                let tlids: Vec<i64> = self.queue.chosen().into_iter().map(|t| t.tlid).collect();
                self.queue.sel.iter_mut().for_each(|s| *s = false);
                self.rpc_fire(
                    "core.tracklist.remove",
                    json!({"criteria": {"tlid": tlids}}),
                );
            }
            K::Char('m') if i > 0 => {
                // Reorder optimistically so the cursor follows the moved song; the
                // tracklist_changed reload then confirms by tlid.
                self.queue.items.swap(i, i - 1);
                self.queue.sel.swap(i, i - 1);
                self.queue.cursor = i - 1;
                self.queue.fix_offset();
                self.rpc_fire(
                    "core.tracklist.move",
                    json!({"start": i, "end": i + 1, "to_position": i - 1}),
                );
            }
            K::Char('n') if i + 1 < self.queue.len() => {
                self.queue.items.swap(i, i + 1);
                self.queue.sel.swap(i, i + 1);
                self.queue.cursor = i + 1;
                self.queue.fix_offset();
                self.rpc_fire(
                    "core.tracklist.move",
                    json!({"start": i, "end": i + 1, "to_position": i + 1}),
                );
            }
            _ => {}
        }
    }

    fn browser_key(&mut self, k: KeyEvent) {
        if matches!(k.code, K::Backspace) {
            return self.browse_up();
        }
        let Some(r) = self.browser.cur().cloned() else {
            return;
        };
        match k.code {
            K::Enter => {
                if r.kind == "parent" {
                    self.browse_up();
                } else if r.is_track() {
                    self.add_uris(vec![r.uri], true, None);
                } else {
                    self.browse_into(r.uri, r.name);
                }
            }
            K::Char(' ') => {
                if r.kind != "parent" {
                    let refs = self
                        .browser
                        .chosen()
                        .into_iter()
                        .filter(|r| r.kind != "parent")
                        .cloned()
                        .collect();
                    self.browser.sel.iter_mut().for_each(|s| *s = false);
                    self.resolve(
                        refs,
                        Purpose::Queue {
                            play: false,
                            at: None,
                        },
                    );
                }
            }
            _ => {}
        }
    }

    fn search_key(&mut self, k: KeyEvent) {
        let Some(item) = self.se.cur() else { return };
        match (item, k.code) {
            (SeItem::Field(i), K::Enter) => {
                let i = *i;
                self.ask(PromptKind::Constraint(i), &format!("{}: ", SE_FIELDS[i]));
            }
            (SeItem::SearchIn, K::Enter) => self.se_in_db = !self.se_in_db,
            (SeItem::Mode, K::Enter) => self.se_mode = (self.se_mode + 1) % SE_MODES.len(),
            (SeItem::Search, K::Enter) => self.se_run(),
            (SeItem::Reset, K::Enter) => self.se_reset(),
            (SeItem::Song(t), K::Enter) => {
                if self.se_in_db {
                    self.add_uris(vec![t.uri.clone()], true, None);
                } else if let Some(tl) = self.queue.items.iter().find(|q| q.track.uri == t.uri) {
                    self.rpc_fire("core.playback.play", json!({"tlid": tl.tlid}));
                }
            }
            (SeItem::Song(_), K::Char(' '))
            | (SeItem::Artist(_), K::Char(' '))
            | (SeItem::Album(_), K::Char(' ')) => {
                let refs = self.chosen_refs();
                self.se.sel.iter_mut().for_each(|s| *s = false);
                self.resolve(
                    refs,
                    Purpose::Queue {
                        play: false,
                        at: None,
                    },
                );
            }
            (SeItem::Artist(r), K::Enter) | (SeItem::Album(r), K::Enter) => {
                let r = r.clone();
                self.goto(Screen::Browser);
                self.browse_into(r.uri, r.name);
            }
            _ => {}
        }
    }

    fn library_key(&mut self, k: KeyEvent) {
        match k.code {
            K::Enter => {
                let refs = self.lib_chosen_refs();
                self.resolve(
                    refs,
                    Purpose::Queue {
                        play: true,
                        at: None,
                    },
                );
            }
            K::Char(' ') => {
                let refs = self.lib_chosen_refs();
                self.with_menu_sel(|m| m.sel.iter_mut().for_each(|s| *s = false));
                self.resolve(
                    refs,
                    Purpose::Queue {
                        play: false,
                        at: None,
                    },
                );
            }
            _ => {}
        }
    }

    fn editor_key(&mut self, k: KeyEvent) {
        let Some(p) = self.playlists.cur().cloned() else {
            return;
        };
        if self.ed_col == 0 {
            match k.code {
                K::Enter => {
                    let refs = self.chosen_refs();
                    self.resolve(
                        refs,
                        Purpose::Queue {
                            play: true,
                            at: None,
                        },
                    );
                }
                K::Char(' ') => {
                    let refs = self.chosen_refs();
                    self.resolve(
                        refs,
                        Purpose::Queue {
                            play: false,
                            at: None,
                        },
                    );
                }
                K::Char('d') | K::Delete => {
                    if !self.owns(&p) {
                        return self
                            .say(format!("Playlist \"{}\" belongs to {}", p.name, p.username));
                    }
                    self.ask(
                        PromptKind::Confirm(Confirm::DeletePlaylist(p.id)),
                        &format!("Delete playlist \"{}\"? [y/n] ", p.name),
                    );
                }
                K::Char('e') => {
                    if !self.owns(&p) {
                        return self
                            .say(format!("Playlist \"{}\" belongs to {}", p.name, p.username));
                    }
                    self.ask(PromptKind::RenamePlaylist(p.id), "Playlist: ");
                }
                _ => {}
            }
            return;
        }
        let i = self.content.cursor;
        let Some(uri) = self.content.cur().cloned() else {
            return;
        };
        match k.code {
            K::Enter => self.add_uris(vec![uri], true, None),
            K::Char(' ') => {
                let refs = self.chosen_refs();
                self.content.sel.iter_mut().for_each(|s| *s = false);
                self.resolve(
                    refs,
                    Purpose::Queue {
                        play: false,
                        at: None,
                    },
                );
            }
            K::Char('d') | K::Delete => {
                if !self.owns(&p) {
                    return self.say(format!("Playlist \"{}\" belongs to {}", p.name, p.username));
                }
                let uris: Vec<String> = self.content.chosen().into_iter().cloned().collect();
                self.content.items.retain(|u| !uris.contains(u));
                self.content.clamp();
                self.content.sel.iter_mut().for_each(|s| *s = false);
                self.pl_cache.insert(p.id, self.content.items.clone());
                let api = self.api.clone();
                self.task(async move {
                    for u in uris {
                        api.remove_track(p.id, &u).await?;
                    }
                    Ok(Msg::Nothing)
                });
            }
            K::Char('m') | K::Char('n') => {
                if !self.owns(&p) {
                    return self.say(format!("Playlist \"{}\" belongs to {}", p.name, p.username));
                }
                let to = if k.code == K::Char('m') {
                    i.checked_sub(1)
                } else {
                    (i + 1 < self.content.len()).then_some(i + 1)
                };
                let Some(to) = to else { return };
                self.content.items.swap(i, to);
                self.content.cursor = to;
                self.content.fix_offset();
                self.pl_cache.insert(p.id, self.content.items.clone());
                let api = self.api.clone();
                self.task(async move {
                    api.reorder_track(p.id, &uri, to).await?;
                    Ok(Msg::Nothing)
                });
            }
            _ => {}
        }
    }

    fn prompt_key(&mut self, k: KeyEvent) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        if let PromptKind::Confirm(_) = p.kind {
            let yes = matches!(k.code, K::Char('y') | K::Char('Y'));
            let PromptKind::Confirm(c) = self.prompt.take().unwrap().kind else {
                unreachable!()
            };
            if yes {
                match c {
                    Confirm::Clear => {
                        self.rpc_fire("core.tracklist.clear", json!({}));
                        self.say("Playlist cleared");
                    }
                    Confirm::DeletePlaylist(id) => {
                        let api = self.api.clone();
                        self.task(async move {
                            api.delete_playlist(id).await?;
                            Ok(Msg::ReloadPlaylists)
                        });
                    }
                }
            }
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            K::Esc => {
                self.prompt = None;
            }
            K::Enter => {
                let p = self.prompt.take().unwrap();
                self.submit(p.kind, p.buf);
            }
            K::Backspace => {
                if let Some(prev) = p.buf[..p.cursor].chars().next_back() {
                    p.cursor -= prev.len_utf8();
                    p.buf.remove(p.cursor);
                }
            }
            K::Delete => {
                if p.cursor < p.buf.len() {
                    p.buf.remove(p.cursor);
                }
            }
            K::Left => {
                if let Some(prev) = p.buf[..p.cursor].chars().next_back() {
                    p.cursor -= prev.len_utf8();
                }
            }
            K::Right => {
                if let Some(next) = p.buf[p.cursor..].chars().next() {
                    p.cursor += next.len_utf8();
                }
            }
            K::Home => p.cursor = 0,
            K::End => p.cursor = p.buf.len(),
            K::Char('u') if ctrl => {
                p.buf.clear();
                p.cursor = 0;
            }
            K::Char('a') if ctrl => p.cursor = 0,
            K::Char('e') if ctrl => p.cursor = p.buf.len(),
            K::Char(c) if !ctrl => {
                p.buf.insert(p.cursor, c);
                p.cursor += c.len_utf8();
            }
            _ => {}
        }
    }

    fn submit(&mut self, kind: PromptKind, buf: String) {
        match kind {
            PromptKind::Username => {
                if buf.is_empty() {
                    return;
                }
                self.ask(PromptKind::Password(buf), "Password: ");
            }
            PromptKind::Password(user) => {
                let api = self.api.clone();
                self.task(async move { Ok(Msg::LoggedIn(api.login(&user, &buf).await?)) });
            }
            PromptKind::Constraint(i) => self.se_fields[i] = buf,
            PromptKind::Find(dir) => {
                self.find = buf;
                self.find_next(dir);
            }
            PromptKind::Jump => {
                let Some(len) = self.current.as_ref().and_then(|c| c.track.length) else {
                    return;
                };
                let b = buf.trim();
                let ms = if let Some(pct) = b.strip_suffix('%') {
                    pct.parse::<f64>()
                        .ok()
                        .map(|p| (len as f64 * p / 100.0) as u64)
                } else if b.contains(':') {
                    let parts: Vec<Option<u64>> = b.split(':').map(|x| x.parse().ok()).collect();
                    parts
                        .iter()
                        .all(Option::is_some)
                        .then(|| parts.iter().fold(0u64, |acc, x| acc * 60 + x.unwrap()) * 1000)
                } else {
                    b.trim_end_matches('s')
                        .parse::<u64>()
                        .ok()
                        .map(|s| s * 1000)
                };
                match ms {
                    Some(ms) if ms < len => {
                        self.set_pos(ms);
                        self.rpc_fire("core.playback.seek", json!({"time_position": ms}));
                    }
                    _ => self.say("Invalid position"),
                }
            }
            PromptKind::SavePlaylist(uris) => {
                if !buf.trim().is_empty() {
                    self.save_playlist(buf.trim().to_string(), uris);
                }
            }
            PromptKind::RenamePlaylist(id) => {
                if buf.trim().is_empty() {
                    return;
                }
                let api = self.api.clone();
                let name = buf.trim().to_string();
                self.task(async move {
                    api.rename_playlist(id, &name).await?;
                    Ok(Msg::ReloadPlaylists)
                });
            }
            PromptKind::AddUri => {
                if !buf.trim().is_empty() {
                    self.add_uris(vec![buf.trim().to_string()], false, None);
                }
            }
            PromptKind::Confirm(_) => {}
        }
    }

    fn popup_key(&mut self, k: KeyEvent) {
        let Some(p) = self.popup.as_mut() else { return };
        match k.code {
            K::Esc | K::Char('q') => self.popup = None,
            K::Down | K::Char('j') => {
                p.cursor = (p.cursor + 1).min(p.items.len().saturating_sub(1))
            }
            K::Up | K::Char('k') => p.cursor = p.cursor.saturating_sub(1),
            K::Home => p.cursor = 0,
            K::End => p.cursor = p.items.len().saturating_sub(1),
            K::Enter => {
                let p = self.popup.take().unwrap();
                let choice = p.items.get(p.cursor).cloned().unwrap_or_default();
                let last = p.items.len().saturating_sub(1);
                match p.kind {
                    PopupKind::AddTo(uris) => {
                        if p.cursor == 0 {
                            let items = vec![
                                "At the end of playlist".to_string(),
                                "At the beginning of playlist".into(),
                                "After current song".into(),
                                "After highlighted item".into(),
                                "Cancel".into(),
                            ];
                            self.popup = Some(Popup {
                                title: "Where?".into(),
                                items,
                                cursor: 0,
                                kind: PopupKind::Where(uris),
                            });
                        } else if p.cursor == last {
                            // Cancel
                        } else if p.cursor == last - 1 {
                            self.ask(PromptKind::SavePlaylist(uris), "Save playlist as: ");
                        } else {
                            let mine: Vec<&Playlist> = self
                                .playlists
                                .items
                                .iter()
                                .filter(|pl| self.owns(pl))
                                .collect();
                            if let Some(pl) = mine.get(p.cursor - 1) {
                                let (id, name) = (pl.id, pl.name.clone());
                                self.add_to_playlist(id, name, uris);
                            }
                        }
                    }
                    PopupKind::Where(uris) => {
                        let at = match choice.as_str() {
                            "At the end of playlist" => None,
                            "At the beginning of playlist" => Some(0),
                            "After current song" => self
                                .current_tlid()
                                .and_then(|c| self.queue.items.iter().position(|t| t.tlid == c))
                                .map(|i| i + 1),
                            "After highlighted item" => {
                                Some((self.queue.cursor + 1).min(self.queue.len()))
                            }
                            _ => return,
                        };
                        self.add_uris(uris, false, at);
                    }
                }
            }
            _ => {}
        }
    }
}

pub struct MenuSel<'a> {
    pub sel: &'a mut Vec<bool>,
}

fn track_ref(t: &Track) -> Ref {
    Ref {
        kind: "track".into(),
        uri: t.uri.clone(),
        name: t.name.clone(),
    }
}

fn uri_ref(u: &str) -> Ref {
    Ref {
        kind: "track".into(),
        uri: u.to_string(),
        name: String::new(),
    }
}

pub fn flatten_lookup(v: &Value) -> Vec<Track> {
    v.as_object()
        .map(|m| {
            m.values()
                .filter_map(Value::as_array)
                .flatten()
                .filter_map(Track::from_value)
                .collect()
        })
        .unwrap_or_default()
}

/// Album/artist/directory/playlist ref -> track uris: lookup first, else browse (depth 3).
async fn expand_ref(rpc: &Rpc, r: &Ref) -> Result<Vec<String>> {
    if r.kind == "album" || r.kind == "playlist" || r.kind == "artist" {
        let v = rpc
            .call("core.library.lookup", json!({"uris": [r.uri]}))
            .await?;
        let uris: Vec<String> = flatten_lookup(&v).into_iter().map(|t| t.uri).collect();
        if !uris.is_empty() {
            return Ok(uris);
        }
    }
    browse_tracks(rpc, &r.uri, 0).await
}

async fn browse_tracks(rpc: &Rpc, uri: &str, depth: u8) -> Result<Vec<String>> {
    let v = rpc.call("core.library.browse", json!({"uri": uri})).await?;
    let refs: Vec<Ref> = serde_json::from_value(v)?;
    let mut out = vec![];
    for r in refs {
        if r.is_track() {
            out.push(r.uri);
        } else if depth < 3 {
            out.extend(Box::pin(browse_tracks(rpc, &r.uri, depth + 1)).await?);
        }
    }
    Ok(out)
}

/// Local "search in current playlist" matcher for the search engine form.
fn se_matches(t: &Track, fields: &[String; 11], exact: bool) -> bool {
    let m = |hay: &str, needle: &str| {
        if exact {
            hay == needle
        } else {
            hay.to_lowercase().contains(&needle.to_lowercase())
        }
    };
    let any = t.classic() + " " + &t.album + " " + &t.uri;
    let values: [&str; 11] = [
        &any, &t.artists, &t.artists, &t.name, &t.album, &t.uri, "", "", "", &t.date, "",
    ];
    fields
        .iter()
        .enumerate()
        .all(|(i, f)| f.trim().is_empty() || m(values[i], f.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_scrolls_classically_and_skips_inactive() {
        let mut m: Menu<i32> = Menu::default();
        m.height = 3;
        m.set((0..10).collect());
        m.mv(4);
        assert_eq!((m.cursor, m.offset), (4, 2));
        m.mv(-4);
        assert_eq!((m.cursor, m.offset), (0, 0));
        m.page(1);
        assert_eq!((m.cursor, m.offset), (3, 3));
        m.cursor = 9;
        m.fix_offset();
        assert_eq!((m.cursor, m.offset), (9, 7));
        m.home();
        m.mv_skip(1, |x| *x == 1 || *x == 2);
        assert_eq!(m.cursor, 3);
        m.toggle_sel();
        assert_eq!(m.chosen(), vec![&3]);
    }

    #[test]
    fn se_local_matching() {
        let t = Track {
            artists: "Nas".into(),
            name: "Represent".into(),
            album: "Illmatic".into(),
            ..Default::default()
        };
        let mut f: [String; 11] = Default::default();
        f[1] = "nas".into();
        assert!(se_matches(&t, &f, false));
        assert!(!se_matches(&t, &f, true));
        f[1] = "Nas".into();
        f[4] = "Illmatic".into();
        assert!(se_matches(&t, &f, true));
    }
}
