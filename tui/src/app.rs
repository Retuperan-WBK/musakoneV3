//! Application state and ncmpcpp key handling. Screens: 1 playlist, 2 now playing,
//! 3 search engine, 4 browser, 5 media library, 6 playlist editor, 7 stats, 8/F1 help,
//! l lyrics.
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
    NowPlaying,
    Browser,
    Search,
    Library,
    Editor,
    Lyrics,
    Stats,
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
    Browsed(String, u64, Result<Vec<Ref>, String>),
    Metadata(Vec<String>, u64, Result<Vec<Track>, String>),
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
    Lyrics(String, Result<Lyrics, String>),
    /// Always completes the loading state, including RPC and HTTP failures.
    Cover(String, Result<Option<Vec<u8>>, String>),
    Stats(Value),
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
    Failed(String),
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
    metadata_pending: HashSet<String>,
    metadata_batches: usize,
    metadata_unavailable: HashSet<String>,

    pub browse_stack: Vec<(Option<String>, String, usize)>,
    pub browser: Menu<Ref>,
    browse_cache: crate::cache::Cache<String, Vec<Ref>>,
    browse_pending: HashSet<String>,
    library_generation: u64,
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
    pub help_offset: usize,
    /// Physical framebuffer size, or an emulated surface for the offline preview.
    pub pixel_size: Option<(u32, u32)>,
    /// Cover and lyric overlays, applied in order after the terminal text flushes.
    pub fb_blits: std::cell::RefCell<Vec<crate::fb::Blit>>,
    pub render_cache: std::cell::RefCell<crate::ui::RenderCache>,
    /// Album art per track uri: Some(img) = decoded, None = fetched but no art.
    pub art_cache: HashMap<String, Option<image::DynamicImage>>,
    art_loading: HashSet<String>,
    /// Stats screen: fetched payload, scope "room"/"me", window days (7/30/0), scroll.
    pub stats: Option<Value>,
    stats_loading: bool,
    pub stats_scope: &'static str,
    pub stats_days: u32,
    pub stats_offset: usize,

    pub prompt: Option<Prompt>,
    pub popup: Option<Popup>,
    /// Song-info overlay (ncmpcpp `i`): label/value pairs, any key closes it
    pub info: Option<(String, Vec<(String, String)>)>,
    pub message: Option<(String, Instant)>,
    find: String,
    /// First endpoint of a Ctrl-V select-range (set on the first press)
    range_anchor: Option<usize>,
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
            metadata_pending: HashSet::new(),
            metadata_batches: 0,
            metadata_unavailable: HashSet::new(),
            browse_stack: vec![(None, String::new(), 0)],
            browser: Menu::default(),
            browse_cache: crate::cache::Cache::new(64, 16 * 1024 * 1024),
            browse_pending: HashSet::new(),
            library_generation: 0,
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
            help_offset: 0,
            pixel_size: None,
            fb_blits: std::cell::RefCell::new(vec![]),
            render_cache: Default::default(),
            art_cache: HashMap::new(),
            art_loading: HashSet::new(),
            stats: None,
            stats_loading: false,
            stats_scope: "room",
            stats_days: 30,
            stats_offset: 0,
            prompt: None,
            popup: None,
            info: None,
            range_anchor: None,
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

    /// At most two metadata batches in flight; never queue an entire directory
    /// ahead of the user's next browse/play command on a serial provider.
    fn lookup_missing(&mut self, uris: &[String]) {
        if !self.connected || self.metadata_batches >= 2 {
            return;
        }
        let missing: Vec<String> = uris
            .iter()
            .filter(|u| !self.tracks.contains_key(*u) && !self.metadata_unavailable.contains(*u))
            .filter(|u| !self.metadata_pending.contains(*u))
            .take((2 - self.metadata_batches) * 100)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for chunk in missing.chunks(100) {
            let rpc = self.rpc.clone();
            let chunk = chunk.to_vec();
            self.metadata_pending.extend(chunk.iter().cloned());
            self.metadata_batches += 1;
            let generation = self.library_generation;
            self.task(async move {
                let result = rpc
                    .call("core.library.lookup", json!({"uris": chunk}))
                    .await
                    .map(|v| flatten_lookup(&v))
                    .map_err(|e| e.to_string());
                Ok(Msg::Metadata(chunk, generation, result))
            });
        }
    }

    /// Names arrive with browse; tags are only needed for the viewport and one
    /// screen ahead. Called after layout so resize and fast scrolling use the
    /// newest viewport rather than enqueueing obsolete pages on every keypress.
    pub fn ensure_visible_metadata(&mut self) {
        let uris = match self.screen {
            Screen::Browser => self
                .browser
                .items
                .iter()
                .skip(self.browser.offset)
                .take(self.browser.height.saturating_mul(2).min(100))
                .filter(|r| r.is_track())
                .map(|r| r.uri.clone())
                .collect::<Vec<_>>(),
            Screen::Editor => self
                .content
                .items
                .iter()
                .skip(self.content.offset)
                .take(self.content.height.saturating_mul(2).min(100))
                .cloned()
                .collect(),
            _ => return,
        };
        self.lookup_missing(&uris);
    }

    fn invalidate_library(&mut self) {
        self.library_generation += 1;
        self.browse_cache.clear();
        self.browse_pending.clear();
        self.metadata_pending.clear();
        self.metadata_batches = 0;
        self.metadata_unavailable.clear();
        self.tracks.clear();
        if self.screen == Screen::Browser {
            self.browse_load();
        }
    }

    /// Toggle a playback mode and announce the new state, like ncmpcpp's statusbar.
    fn toggle_mode(&mut self, method: &'static str, current: bool) {
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
        if let Some(items) = self.browse_cache.get(&key) {
            self.browse_show(items);
            return;
        }
        self.browse_loading = true;
        if !self.browse_pending.insert(key.clone()) {
            return;
        }
        let rpc = self.rpc.clone();
        let generation = self.library_generation;
        self.task(async move {
            let result = async {
                let v = rpc.call("core.library.browse", json!({"uri": uri})).await?;
                Ok::<Vec<Ref>, anyhow::Error>(serde_json::from_value(v)?)
            }
            .await
            .map_err(|e| e.to_string());
            Ok(Msg::Browsed(key, generation, result))
        });
    }

    fn browse_show(&mut self, mut items: Vec<Ref>) {
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
        let cursor = self.browser.cursor;
        self.browser.set(items);
        self.browser.cursor = cursor;
        self.browser.clamp();
        self.browse_loading = false;
    }

    fn cache_browse(&mut self, uri: String, refs: Vec<Ref>) {
        let bytes = uri.len()
            + refs
                .iter()
                .map(|r| std::mem::size_of::<Ref>() + r.uri.len() + r.kind.len() + r.name.len())
                .sum::<usize>();
        self.browse_cache.insert(uri, refs, bytes);
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
        self.se.home();
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

    // ---------- artwork / lyrics ----------

    /// Only load current-track media while one of its display screens is visible.
    fn ensure_current_media(&mut self) {
        if !matches!(self.screen, Screen::NowPlaying | Screen::Lyrics) {
            return;
        }
        let Some(uri) = self.current.as_ref().map(|c| c.track.uri.clone()) else {
            self.lyrics = LyricsState::Idle;
            return;
        };
        self.ensure_lyrics();
        if self.screen == Screen::NowPlaying {
            self.ensure_art(uri);
        }
    }

    /// Prefetch art for the song under the search cursor (for the preview panel).
    fn ensure_search_art(&mut self) {
        if self.screen != Screen::Search {
            return;
        }
        if let Some(SeItem::Song(t)) = self.se.cur() {
            let uri = t.uri.clone();
            self.ensure_art(uri);
        }
    }

    /// Fetch the stats payload for the current scope/window if not already loaded.
    fn ensure_stats(&mut self) {
        if self.stats.is_some() || self.stats_loading {
            return;
        }
        self.stats_loading = true;
        let api = self.api.clone();
        let (scope, days) = (self.stats_scope, self.stats_days);
        self.task(async move { Ok(Msg::Stats(api.stats(scope, days).await?)) });
    }

    fn reload_stats(&mut self) {
        self.stats = None;
        self.stats_loading = false;
        self.stats_offset = 0;
        self.ensure_stats();
    }

    /// Left/Right on the stats screen cycles the time window 7d -> 30d -> all.
    fn cycle_stats_range(&mut self, dir: isize) {
        let order = [7u32, 30, 0];
        let i = order
            .iter()
            .position(|d| *d == self.stats_days)
            .unwrap_or(1) as isize;
        self.stats_days = order[(i + dir).rem_euclid(order.len() as isize) as usize];
        self.reload_stats();
    }

    fn toggle_stats_scope(&mut self) {
        self.stats_scope = if self.stats_scope == "room" {
            "me"
        } else {
            "room"
        };
        self.reload_stats();
    }

    /// Fetch and decode a track's album art into art_cache (Mopidy get_images → URL → bytes).
    fn ensure_art(&mut self, uri: String) {
        if self.art_cache.contains_key(&uri) || self.art_loading.contains(&uri) {
            return;
        }
        // Keep the cache bounded; a session won't hover thousands of tracks.
        if self.art_cache.len() > 64 {
            self.art_cache.clear();
        }
        self.art_loading.insert(uri.clone());
        let rpc = self.rpc.clone();
        let api = self.api.clone();
        let roots = self.browse_cache.get("");
        self.task(async move {
            let result: Result<Option<Vec<u8>>> = async {
                match cover_image_url(&rpc, &uri, roots).await? {
                    Some(url) => Ok(Some(api.fetch_bytes(&url).await?)),
                    None => Ok(None),
                }
            }
            .await;
            Ok(Msg::Cover(uri, result.map_err(|e| e.to_string())))
        });
    }

    fn ensure_lyrics(&mut self) {
        let Some(cur) = self.current.as_ref().map(|c| c.track.clone()) else {
            return;
        };
        match &self.lyrics {
            LyricsState::Loaded(u, _) | LyricsState::Fetching(u) | LyricsState::Failed(u)
                if *u == cur.uri =>
            {
                return;
            }
            _ => {}
        }
        self.lyrics = LyricsState::Fetching(cur.uri.clone());
        let api = self.api.clone();
        let dir = self.lyrics_dir.clone();
        self.task(async move {
            let uri = cur.uri.clone();
            let l = api.lyrics(&cur, &dir).await.map_err(|e| e.to_string());
            Ok(Msg::Lyrics(uri, l))
        });
    }

    /// Index of the lyric line active at the current position (synced lyrics only).
    pub fn lyrics_active(&self) -> Option<usize> {
        let LyricsState::Loaded(uri, lines) = &self.lyrics else {
            return None;
        };
        if self.current.as_ref()?.track.uri != *uri {
            return None;
        }
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

    pub fn current_lyric(&self) -> Option<&str> {
        let LyricsState::Loaded(_, lines) = &self.lyrics else {
            return None;
        };
        Some(lines.get(self.lyrics_active()?)?.1.as_str())
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
        self.ensure_current_media();
        if self.screen == Screen::Stats {
            self.ensure_stats();
        }
        if self.screen == Screen::Search {
            self.ensure_search_art();
        }
    }

    pub fn on_connected(&mut self) {
        self.connected = true;
        self.invalidate_library();
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
            "library_changed" => self.invalidate_library(),
            "mopidy_connected" => {
                self.mopidy_ok = true;
                self.invalidate_library();
                self.refresh_all();
            }
            "mopidy_disconnected" => {
                self.mopidy_ok = false;
                self.say("Connection to Mopidy lost");
            }
            _ => {}
        }
        self.ensure_current_media();
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
                self.ensure_current_media();
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
            Msg::Browsed(uri, generation, result) => {
                if generation != self.library_generation {
                    return;
                }
                self.browse_pending.remove(&uri);
                let current = self.browse_uri().unwrap_or_default() == uri;
                match result {
                    Ok(refs) => {
                        if current {
                            self.browse_show(refs.clone());
                        }
                        self.cache_browse(uri, refs);
                    }
                    Err(error) if current => {
                        self.browse_loading = false;
                        self.say(format!("Could not browse: {error}"));
                    }
                    Err(_) => {}
                }
            }
            Msg::Metadata(uris, generation, result) => {
                if generation != self.library_generation {
                    return;
                }
                self.metadata_batches = self.metadata_batches.saturating_sub(1);
                for uri in &uris {
                    self.metadata_pending.remove(uri);
                }
                if let Ok(tracks) = result {
                    self.remember(tracks);
                }
                // Missing or failed metadata keeps its browse name and is retried
                // after a library refresh/reconnect, not on every frame.
                self.metadata_unavailable
                    .extend(uris.into_iter().filter(|u| !self.tracks.contains_key(u)));
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
            Msg::Lyrics(uri, result) => {
                if matches!(&self.lyrics, LyricsState::Fetching(u) if *u == uri) {
                    self.lyrics = match result {
                        Ok(l) => LyricsState::Loaded(uri, l.lines()),
                        Err(error) => {
                            self.say(format!("Could not load lyrics: {error}"));
                            LyricsState::Failed(uri)
                        }
                    };
                }
            }
            Msg::Cover(uri, result) => {
                self.render_cache.borrow_mut().clear_covers();
                self.art_loading.remove(&uri);
                let img = match result {
                    Ok(bytes) => bytes.and_then(|b| image::load_from_memory(&b).ok()),
                    Err(error) => {
                        self.say(format!("Could not load album art: {error}"));
                        None
                    }
                };
                self.art_cache.insert(uri, img);
            }
            Msg::Stats(v) => {
                self.stats_loading = false;
                self.stats = Some(v);
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
        if self.info.is_some() {
            // Any key dismisses the song-info overlay.
            self.info = None;
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let columns = matches!(self.screen, Screen::Library | Screen::Editor)
            || (self.screen == Screen::Search && self.se_has_results());
        match k.code {
            K::Char('v') if ctrl => self.select_range(),
            K::Char('q') => self.quit = true,
            K::Char('i') => self.show_song_info(),
            K::Char('B') => self.select_album(),
            K::Char('[') => self.jump_group(-1, false),
            K::Char(']') => self.jump_group(1, false),
            K::Char('{') => self.jump_group(-1, true),
            K::Char('}') => self.jump_group(1, true),
            K::Char('8') | K::F(1) => self.goto(Screen::Help),
            K::Char('1') => self.goto(Screen::Playlist),
            K::Char('2') => self.goto(Screen::NowPlaying),
            K::Char('3') => {
                if self.screen == Screen::Search {
                    self.se_reset();
                }
                self.goto(Screen::Search);
            }
            K::Char('4') => self.goto(Screen::Browser),
            K::Char('5') => self.goto(Screen::Library),
            K::Char('6') => self.goto(Screen::Editor),
            K::Char('7') => self.goto(Screen::Stats),
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
            K::Left | K::Right if self.screen == Screen::Browser => self.browser_key(k),
            K::Left if self.screen == Screen::Stats => self.cycle_stats_range(-1),
            K::Right if self.screen == Screen::Stats => self.cycle_stats_range(1),
            K::Right if columns => self.next_column(1),
            K::Left if columns => self.next_column(-1),
            K::Right | K::Char('+') => self.set_volume(self.volume + VOL_STEP),
            K::Left | K::Char('-') => self.set_volume(self.volume - VOL_STEP),
            K::Char('r') => self.toggle_mode("core.tracklist.set_repeat", self.repeat),
            K::Char('z') => self.toggle_mode("core.tracklist.set_random", self.random),
            K::Char('y') => self.toggle_mode("core.tracklist.set_single", self.single),
            K::Char('R') => self.toggle_mode("core.tracklist.set_consume", self.consume),
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
            K::Char('u') if self.screen == Screen::Stats => self.reload_stats(),
            K::Char('u') => {
                self.rpc_fire("core.library.refresh", json!({}));
                self.invalidate_library();
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
                    if self.screen != Screen::Lyrics {
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
            K::Char('`') if matches!(self.screen, Screen::NowPlaying | Screen::Lyrics) => {
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
        self.ensure_current_media();
        if s == Screen::Stats {
            self.ensure_stats();
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
            Screen::NowPlaying => self.current.iter().map(|c| track_ref(&c.track)).collect(),
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
            Screen::Lyrics | Screen::Help | Screen::Stats => vec![],
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
    /// The Track represented by row `i` on the active screen, if it is a song row.
    fn row_track(&self, i: usize) -> Option<Track> {
        match self.screen {
            Screen::Playlist => self.queue.items.get(i).map(|t| t.track.clone()),
            Screen::Browser => {
                let r = self.browser.items.get(i)?;
                r.is_track()
                    .then(|| self.track_of(&r.uri).cloned())
                    .flatten()
            }
            Screen::Search => match self.se.items.get(i)? {
                SeItem::Song(t) => Some(t.clone()),
                _ => None,
            },
            Screen::Library if self.lib_col == 2 => self.lib_songs.items.get(i).cloned(),
            Screen::Editor if self.ed_col == 1 => self
                .content
                .items
                .get(i)
                .and_then(|u| self.track_of(u).cloned()),
            _ => None,
        }
    }

    /// Song-info overlay (ncmpcpp `i`): the cursor's song, or the playing one.
    fn show_song_info(&mut self) {
        let (_, cur) = self.active_len_cursor();
        let Some(t) = self
            .row_track(cur)
            .or_else(|| self.current.as_ref().map(|c| c.track.clone()))
        else {
            self.say("No song here");
            return;
        };
        let mut v = vec![("Title".into(), t.name.clone())];
        if !t.artists.is_empty() {
            v.push(("Artist".into(), t.artists.clone()));
        }
        if !t.album.is_empty() {
            v.push(("Album".into(), t.album.clone()));
        }
        if !t.date.is_empty() {
            v.push(("Date".into(), t.date.clone()));
        }
        if let Some(n) = t.track_no {
            v.push(("Track".into(), n.to_string()));
        }
        if t.length.is_some() {
            v.push(("Length".into(), t.duration()));
        }
        v.push(("URI".into(), t.uri.clone()));
        self.ensure_art(t.uri.clone());
        self.info = Some((t.uri, v));
    }

    /// ncmpcpp Ctrl-V: first press sets an endpoint, second selects the range.
    fn select_range(&mut self) {
        let (_, cur) = self.active_len_cursor();
        match self.range_anchor.take() {
            None => {
                self.range_anchor = Some(cur);
                self.say("Range selection started; press Ctrl-V again at the other end");
            }
            Some(a) => {
                let (lo, hi) = (a.min(cur), a.max(cur));
                self.with_menu_sel(|m| {
                    for s in m.sel.iter_mut().take(hi + 1).skip(lo) {
                        *s = true;
                    }
                });
                self.say(format!("Selected {} items", hi - lo + 1));
            }
        }
    }

    /// ncmpcpp B: select the run of rows around the cursor that share its album.
    fn select_album(&mut self) {
        let (len, cur) = self.active_len_cursor();
        let Some(t) = self.row_track(cur) else {
            self.say("Not a song");
            return;
        };
        if t.album.is_empty() {
            self.say("No album tag");
            return;
        }
        let same = |app: &App, i: usize| app.row_track(i).map(|x| x.album) == Some(t.album.clone());
        let mut lo = cur;
        while lo > 0 && same(self, lo - 1) {
            lo -= 1;
        }
        let mut hi = cur;
        while hi + 1 < len && same(self, hi + 1) {
            hi += 1;
        }
        self.with_menu_sel(|m| {
            for s in m.sel.iter_mut().take(hi + 1).skip(lo) {
                *s = true;
            }
        });
        self.say(format!("Selected album ({} tracks)", hi - lo + 1));
    }

    /// ncmpcpp [ ] { }: move the cursor to the previous/next album or artist boundary.
    fn jump_group(&mut self, dir: isize, by_artist: bool) {
        let (len, cur) = self.active_len_cursor();
        let key = |t: &Track| {
            if by_artist {
                t.artists.clone()
            } else {
                t.album.clone()
            }
        };
        let Some(cur_t) = self.row_track(cur) else {
            return;
        };
        let cur_key = key(&cur_t);
        let mut i = cur as isize;
        loop {
            i += dir;
            if i < 0 || i >= len as isize {
                return;
            }
            if self.row_track(i as usize).map(|t| key(&t)) != Some(cur_key.clone()) {
                break;
            }
        }
        // Going up, land on the first row of the previous group, not its last.
        if dir < 0 {
            let gk = self.row_track(i as usize).map(|t| key(&t));
            while i > 0 && self.row_track((i - 1) as usize).map(|t| key(&t)) == gk {
                i -= 1;
            }
        }
        self.set_cursor(i as usize);
    }

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
            Screen::Search => {
                self.se.fix_offset();
                self.ensure_search_art();
            }
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
            Screen::NowPlaying | Screen::Lyrics | Screen::Help | Screen::Stats => {}
        }
    }

    /// Movement keys shared by every list screen.
    fn mv(&mut self, d: isize, page: bool) {
        match self.screen {
            Screen::Lyrics => return,
            Screen::Help => {
                let step = if page { 10 } else { 1 };
                self.help_offset = (self.help_offset as isize + d * step)
                    .clamp(0, crate::ui::help_len() as isize - 1)
                    as usize;
                return;
            }
            Screen::Stats => {
                let step = if page { 10 } else { 1 };
                self.stats_offset = (self.stats_offset as isize + d * step).max(0) as usize;
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
            Screen::Stats => {
                // Space or Tab-like toggles room/me; u refreshes.
                match k.code {
                    K::Char(' ') | K::Enter => self.toggle_stats_scope(),
                    _ => {}
                }
            }
            Screen::NowPlaying | Screen::Lyrics | Screen::Help => {}
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
        if matches!(k.code, K::Left | K::Backspace) {
            return self.browse_up();
        }
        let Some(r) = self.browser.cur().cloned() else {
            return;
        };
        match k.code {
            K::Right if !r.is_track() && r.kind != "parent" => {
                self.browse_into(r.uri, r.name);
            }
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
            (SeItem::Song(t), K::Char(' ')) => {
                // Toggle the song in the queue: add if absent, remove every copy if
                // present (so it can't be queued twice). Update queue_uris optimistically
                // so the row lights up instantly, then advance to the next song.
                let uri = t.uri.clone();
                if self.queue_uris.remove(&uri) {
                    self.rpc_fire("core.tracklist.remove", json!({"criteria": {"uri": [uri]}}));
                } else {
                    self.queue_uris.insert(uri.clone());
                    self.add_uris(vec![uri], false, None);
                }
                self.se.mv_skip(1, SeItem::inactive);
            }
            (SeItem::Artist(_), K::Char(' ')) | (SeItem::Album(_), K::Char(' ')) => {
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

/// Files and Local media use different URIs for the same recording. Resolve a
/// Files path relative to a configured music root, never a hardcoded mount path.
fn local_art_uri(uri: &str, root: &str) -> Option<String> {
    let file = reqwest::Url::parse(uri).ok()?;
    let base = reqwest::Url::parse(root).ok()?;
    if file.scheme() != "file"
        || base.scheme() != "file"
        || file.host_str() != base.host_str()
        || file.query().is_some()
        || file.fragment().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return None;
    }
    let relative = file
        .path()
        .strip_prefix(base.path().trim_end_matches('/'))?
        .strip_prefix('/')?;
    if relative.is_empty() {
        return None;
    }
    Some(format!("local:track:{relative}"))
}

fn largest_image(v: &Value, uri: &str) -> Option<String> {
    v.get(uri)?
        .as_array()?
        .iter()
        .filter(|img| img.get("uri").and_then(Value::as_str).is_some())
        .max_by_key(|img| img.get("width").and_then(Value::as_u64).unwrap_or(0))?
        .get("uri")?
        .as_str()
        .map(String::from)
}

async fn cover_image_url(
    rpc: &Rpc,
    uri: &str,
    cached_roots: Option<Vec<Ref>>,
) -> Result<Option<String>> {
    let v = rpc
        .call("core.library.get_images", json!({"uris": [uri]}))
        .await?;
    let native = largest_image(&v, uri);
    if native.is_some() || !uri.starts_with("file:") {
        return Ok(native);
    }

    // Mopidy's File backend supplies no images. Ask the indexed Local backend
    // for the equivalent relative track URI without scanning any audio files.
    let mut roots: Vec<Ref> = match cached_roots {
        Some(roots) => roots,
        None => serde_json::from_value(
            rpc.call("core.library.browse", json!({"uri": null}))
                .await?,
        )?,
    };
    if !roots.iter().any(|r| r.uri.starts_with("local:")) {
        return Ok(None);
    }
    if roots.iter().any(|r| r.uri == "file:root") {
        let dirs: Vec<Ref> = serde_json::from_value(
            rpc.call("core.library.browse", json!({"uri": "file:root"}))
                .await?,
        )?;
        roots.extend(dirs);
    }
    roots.sort_by_key(|r| std::cmp::Reverse(r.uri.len()));
    let mut seen = HashSet::new();
    let aliases: Vec<String> = roots
        .iter()
        .filter_map(|r| local_art_uri(uri, &r.uri))
        .filter(|alias| seen.insert(alias.clone()))
        .collect();
    if aliases.is_empty() {
        return Ok(None);
    }
    let v = rpc
        .call("core.library.get_images", json!({"uris": aliases}))
        .await?;
    Ok(aliases.iter().find_map(|alias| largest_image(&v, alias)))
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

    fn test_app() -> App {
        let (tx, _) = mpsc::unbounded_channel();
        let mut app = App::new(
            Rpc::offline(),
            Arc::new(Api::new("http://127.0.0.1:1".into(), None)),
            tx,
            PathBuf::new(),
            PathBuf::new(),
            false,
            None,
        );
        app.prompt = None;
        app
    }

    fn render_app(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| crate::ui::draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn screen_text(buffer: &ratatui::buffer::Buffer) -> String {
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn current_track() -> TlTrack {
        TlTrack {
            tlid: 1,
            track: Track {
                uri: "local:track:now-playing".into(),
                name: "Current song".into(),
                artists: "Soloist".into(),
                album_artists: "Orchestra".into(),
                album: "Live recording".into(),
                length: Some(120_000),
                ..Default::default()
            },
        }
    }

    fn scripted_rpc(
        script: Vec<(&'static str, Value, Value)>,
    ) -> (Arc<Rpc>, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tokio_tungstenite::tungstenite::accept(stream).unwrap();
            for (method, params, result) in script {
                let message = socket.read().unwrap();
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                assert_eq!(request["method"], method);
                assert_eq!(request["params"], params);
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        json!({"jsonrpc": "2.0", "id": request["id"], "result": result})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
            }
        });
        let (events, _) = mpsc::unbounded_channel();
        (
            Rpc::start(format!("ws://{address}/ws"), None, events),
            server,
        )
    }

    #[test]
    fn files_artwork_alias_preserves_encoded_paths_and_respects_root_boundaries() {
        assert_eq!(
            local_art_uri(
                "file:///mnt/Orchestra/Retupera%CC%88n%20WBK/Album/01%20Song%23.mp3",
                "file:///mnt/Orchestra/",
            )
            .as_deref(),
            Some("local:track:Retupera%CC%88n%20WBK/Album/01%20Song%23.mp3"),
        );
        for uri in [
            "file:///mnt/Orchestra-other/Album/Song.mp3",
            "file:///mnt/Orchestra",
            "file:///mnt/Orchestra/Album/Song.mp3?other=true",
            "file:///mnt/Orchestra/Album/Song.mp3#fragment",
            "file://other-host/mnt/Orchestra/Album/Song.mp3",
            "https://example.com/mnt/Orchestra/Album/Song.mp3",
        ] {
            assert_eq!(local_art_uri(uri, "file:///mnt/Orchestra"), None);
        }
    }

    #[tokio::test]
    async fn files_artwork_uses_indexed_local_cover_without_changing_track_uri() {
        let uri = "file:///mnt/Orchestra/Artist/Album/Song.mp3";
        let local = "local:track:Artist/Album/Song.mp3";
        let roots = serde_json::from_value(json!([
            {"type": "directory", "uri": "file:///mnt/Orchestra", "name": "Files"},
            {"type": "directory", "uri": "local:directory", "name": "Local media"}
        ]))
        .unwrap();
        let (rpc, server) = scripted_rpc(vec![
            (
                "core.library.get_images",
                json!({"uris": [uri]}),
                json!({uri: []}),
            ),
            (
                "core.library.get_images",
                json!({"uris": [local]}),
                json!({local: [
                    {"uri": "/local/small.jpg", "width": 100},
                    {"uri": "/local/cover.png", "width": 600}
                ]}),
            ),
        ]);
        assert_eq!(
            cover_image_url(&rpc, uri, Some(roots))
                .await
                .unwrap()
                .as_deref(),
            Some("/local/cover.png")
        );
        server.join().unwrap();
    }

    #[tokio::test]
    async fn files_artwork_discovers_multiple_configured_music_roots() {
        let uri = "file:///other/music/Album/Song.mp3";
        let local = "local:track:Album/Song.mp3";
        let (rpc, server) = scripted_rpc(vec![
            (
                "core.library.get_images",
                json!({"uris": [uri]}),
                json!({uri: []}),
            ),
            (
                "core.library.browse",
                json!({"uri": null}),
                json!([
                    {"type": "directory", "uri": "file:root", "name": "Files"},
                    {"type": "directory", "uri": "local:directory", "name": "Local media"}
                ]),
            ),
            (
                "core.library.browse",
                json!({"uri": "file:root"}),
                json!([
                    {"type": "directory", "uri": "file:///mnt/music", "name": "Music"},
                    {"type": "directory", "uri": "file:///other/music", "name": "More music"}
                ]),
            ),
            (
                "core.library.get_images",
                json!({"uris": [local]}),
                json!({local: [
                    {"uri": "/local/cover.jpg", "width": 600}
                ]}),
            ),
        ]);
        assert_eq!(
            cover_image_url(&rpc, uri, None).await.unwrap().as_deref(),
            Some("/local/cover.jpg")
        );
        server.join().unwrap();
    }

    #[tokio::test]
    async fn provider_artwork_urls_do_not_trigger_files_fallback() {
        let uri = "tidal:track:123";
        let (rpc, server) = scripted_rpc(vec![(
            "core.library.get_images",
            json!({"uris": [uri]}),
            json!({uri: [
                {"uri": "https://images.example/cover.jpg", "width": 600}
            ]}),
        )]);
        assert_eq!(
            cover_image_url(&rpc, uri, None).await.unwrap().as_deref(),
            Some("https://images.example/cover.jpg")
        );
        server.join().unwrap();
    }

    #[tokio::test]
    async fn search_reset_clears_the_form_and_returns_to_any() {
        let mut app = test_app();
        app.screen = Screen::Search;
        app.se_fields[0] = "Current song".into();
        app.se_tracks = vec![current_track().track];
        app.se.height = 4;
        app.se_rebuild(false);
        app.se.cursor = app
            .se
            .items
            .iter()
            .position(|item| matches!(item, SeItem::Reset))
            .unwrap();
        app.se.fix_offset();
        assert!(app.se.offset > 0);

        app.key(KeyEvent::new(K::Enter, KeyModifiers::NONE));
        assert_eq!((app.se.cursor, app.se.offset), (0, 0));
        assert!(matches!(app.se.cur(), Some(SeItem::Field(0))));
        assert!(app.se_fields.iter().all(String::is_empty));
        assert!(!app.se_has_results());
    }

    #[tokio::test]
    async fn repeating_search_shortcut_resets_search_and_focuses_any() {
        let mut app = test_app();
        app.screen = Screen::Search;
        app.se_fields[0] = "Current song".into();
        app.se_tracks = vec![current_track().track];
        app.se.height = 4;
        app.se_rebuild(false);
        app.se.sel[app.se.cursor] = true;
        assert!(app.se.offset > 0);

        app.key(KeyEvent::new(K::Char('3'), KeyModifiers::NONE));
        assert_eq!((app.se.cursor, app.se.offset), (0, 0));
        assert!(matches!(app.se.cur(), Some(SeItem::Field(0))));
        assert!(app.se_fields.iter().all(String::is_empty));
        assert!(!app.se_has_results());
        assert_eq!(app.se.len(), App::se_static().len());
        assert!(!app.se.any_sel());
    }

    #[tokio::test]
    async fn switching_back_to_search_preserves_cursor_until_three_is_pressed_again() {
        let mut app = test_app();
        app.screen = Screen::Search;
        app.se.cursor = 4;
        app.se_fields[0] = "Current song".into();
        app.key(KeyEvent::new(K::Char('1'), KeyModifiers::NONE));
        app.key(KeyEvent::new(K::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::Search);
        assert_eq!(app.se.cursor, 4);
        assert_eq!(app.se_fields[0], "Current song");
        app.key(KeyEvent::new(K::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.se.cursor, 0);
        assert!(app.se_fields.iter().all(String::is_empty));
        app.key(KeyEvent::new(K::Enter, KeyModifiers::NONE));
        app.key(KeyEvent::new(K::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.prompt.as_ref().unwrap().buf, "3");
    }

    #[tokio::test]
    async fn numbered_screens_use_the_new_layout() {
        let mut app = test_app();
        app.stats = Some(json!({}));
        for (key, screen) in [
            ('1', Screen::Playlist),
            ('2', Screen::NowPlaying),
            ('3', Screen::Search),
            ('4', Screen::Browser),
            ('5', Screen::Library),
            ('6', Screen::Editor),
            ('7', Screen::Stats),
            ('8', Screen::Help),
        ] {
            app.key(KeyEvent::new(K::Char(key), KeyModifiers::NONE));
            assert_eq!(app.screen, screen);
        }
        app.key(KeyEvent::new(K::Char('1'), KeyModifiers::NONE));
        app.key(KeyEvent::new(K::Tab, KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::Browser);
        app.key(KeyEvent::new(K::BackTab, KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::Playlist);
    }

    #[tokio::test]
    async fn now_playing_renders_a_large_cover_album_artist_and_only_current_synced_line() {
        let mut app = test_app();
        app.screen = Screen::NowPlaying;
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.art_cache.insert(
            uri.clone(),
            Some(image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                2,
                2,
                image::Rgb([30, 100, 180]),
            ))),
        );
        app.lyrics = LyricsState::Loaded(
            uri.clone(),
            vec![
                (Some(1_000), "Previous lyric".into()),
                (Some(2_000), "The current lyric line".into()),
                (Some(4_000), "Next lyric".into()),
            ],
        );
        app.set_pos(2_500);
        let buffer = render_app(&mut app, 120, 40);
        let text = screen_text(&buffer);
        assert!(text.contains("Now playing"));
        assert!(text.contains("Current song"));
        assert!(text.contains("Orchestra"));
        assert!(text.contains("Live recording"));
        assert!(text.contains("Track artist: Soloist"));
        assert_eq!(app.current_lyric(), Some("The current lyric line"));
        assert!(!text.contains("Previous lyric"));
        assert!(!text.contains("Next lyric"));
        let art_cells = buffer
            .content
            .iter()
            .filter(|cell| cell.bg != ratatui::style::Color::Reset)
            .count();
        assert!(
            art_cells > 500,
            "expected a large cover, got {art_cells} image cells"
        );
        let colored: Vec<(usize, usize)> = buffer
            .content
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.bg != ratatui::style::Color::Reset)
            .map(|(i, _)| (i % 120, i / 120))
            .collect();
        let left = colored.iter().map(|(x, _)| *x).min().unwrap();
        let right = colored.iter().map(|(x, _)| *x).max().unwrap();
        let bottom = colored.iter().map(|(_, y)| *y).max().unwrap();
        assert!(
            (left + right).abs_diff(119) <= 1,
            "cover must be horizontally centered"
        );
        let row_of = |text: &str| {
            buffer
                .content
                .chunks(120)
                .enumerate()
                .skip(3)
                .find(|(_, row)| {
                    row.iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>()
                        .contains(text)
                })
                .map(|(y, _)| y)
                .unwrap()
        };
        assert!(bottom < row_of("Current song"));
        assert!(row_of("Current song") < row_of("Orchestra"));
        assert!(row_of("Orchestra") < row_of("Live recording"));
        assert!(text.contains("The current lyric line"));
        let lyric_row = row_of("The current lyric line");
        assert!(row_of("Live recording") < lyric_row);
        let caption = &buffer.content[lyric_row * 120..(lyric_row + 1) * 120];
        assert!(!caption.iter().any(|cell| cell.symbol() == "█"));
        for cell in caption.iter().filter(|cell| cell.symbol() != " ") {
            assert_eq!(cell.fg, ratatui::style::Color::LightYellow);
            assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
        }
        assert!(app.fb_blits.borrow().is_empty());

        app.lyrics = LyricsState::Loaded(uri, vec![(None, "Plain lyrics must stay hidden".into())]);
        let text = screen_text(&render_app(&mut app, 120, 40));
        assert!(!text.contains("Plain lyrics must stay hidden"));
        assert!(text.contains("No synced lyrics available"));
    }

    #[tokio::test]
    async fn live_lyrics_use_normal_text_with_current_and_two_upcoming_lines_only() {
        let mut app = test_app();
        app.screen = Screen::Lyrics;
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.lyrics = LyricsState::Loaded(
            uri,
            vec![
                (Some(0), "Beginning of the song".into()),
                (Some(1_000), "Earlier verse".into()),
                (Some(2_000), "Previous cue".into()),
                (Some(3_000), "We are here".into()),
                (Some(4_000), "Next cue".into()),
                (Some(5_000), "End of the song".into()),
                (Some(6_000), "Later unseen verse".into()),
            ],
        );
        app.set_pos(3_500);
        let buffer = render_app(&mut app, 120, 40);
        let text = screen_text(&buffer);
        assert!(text.contains("Live lyrics"));
        assert!(!text.contains("Previous cue"));
        assert!(!text.contains("Beginning of the song"));
        assert!(!text.contains("Earlier verse"));
        assert!(!text.contains("Later unseen verse"));
        assert!(!text.contains('█'));
        let lyric_rows: Vec<_> = buffer
            .content
            .chunks(120)
            .enumerate()
            .filter_map(|(y, row)| {
                let line: String = row.iter().map(|cell| cell.symbol()).collect();
                let line = line.trim();
                matches!(line, "We are here" | "Next cue" | "End of the song")
                    .then(|| (y, line.to_string()))
            })
            .collect();
        assert_eq!(
            lyric_rows
                .iter()
                .map(|(_, line)| line.as_str())
                .collect::<Vec<_>>(),
            vec!["We are here", "Next cue", "End of the song"]
        );
        assert!(lyric_rows[0].0 > 3);
        assert!(lyric_rows[2].0 < 38);
        for (i, (y, _)) in lyric_rows.iter().enumerate() {
            for cell in buffer.content[y * 120..(y + 1) * 120]
                .iter()
                .filter(|cell| cell.symbol() != " ")
            {
                assert_eq!(
                    cell.fg,
                    if i == 0 {
                        ratatui::style::Color::LightYellow
                    } else {
                        ratatui::style::Color::Gray
                    }
                );
                assert_eq!(
                    cell.modifier.contains(ratatui::style::Modifier::BOLD),
                    i == 0
                );
            }
        }
        assert!(app.fb_blits.borrow().is_empty());

        for key in [
            K::Down,
            K::Up,
            K::PageDown,
            K::PageUp,
            K::Char('j'),
            K::Char('k'),
        ] {
            app.key(KeyEvent::new(key, KeyModifiers::NONE));
            assert_eq!(app.current_lyric(), Some("We are here"));
        }
        assert_eq!(render_app(&mut app, 120, 40), buffer);
        app.on_event(json!({"event": "seeked", "time_position": 4_500}));
        assert_eq!(app.current_lyric(), Some("Next cue"));
        assert_ne!(render_app(&mut app, 120, 40), buffer);
    }

    #[tokio::test]
    async fn live_lyrics_preview_first_two_lines_before_the_first_timestamp() {
        let mut app = test_app();
        app.screen = Screen::Lyrics;
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.lyrics = LyricsState::Loaded(
            uri,
            vec![
                (Some(10_000), "第一行".into()),
                (Some(20_000), "第二行".into()),
                (Some(30_000), "第三行".into()),
            ],
        );
        app.set_pos(0);
        let text = screen_text(&render_app(&mut app, 120, 40)).replace(' ', "");
        assert!(text.contains('♪'));
        assert!(text.contains("第一行"));
        assert!(text.contains("第二行"));
        assert!(!text.contains("第三行"));
        app.set_pos(10_000);
        let text = screen_text(&render_app(&mut app, 120, 40)).replace(' ', "");
        assert!(!text.contains('♪'));
        for line in ["第一行", "第二行", "第三行"] {
            assert!(text.contains(line));
        }
    }

    #[tokio::test]
    async fn live_lyrics_handle_plain_text_fallback_and_narrow_windows() {
        let mut app = test_app();
        app.screen = Screen::Lyrics;
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.lyrics = LyricsState::Loaded(uri.clone(), vec![(Some(0), "你好，世界".into())]);
        assert!(screen_text(&render_app(&mut app, 80, 24))
            .replace(' ', "")
            .contains("你好，世界"));
        app.lyrics = LyricsState::Loaded(
            uri.clone(),
            vec![(None, "Entire unsynced song must stay hidden".into())],
        );
        let text = screen_text(&render_app(&mut app, 80, 24));
        assert!(text.contains("No synced lyrics available"));
        assert!(!text.contains("Entire unsynced song"));
        app.lyrics = LyricsState::Loaded(
            uri.clone(),
            vec![(
                Some(0),
                "A long lyric that must fit without losing the current line".into(),
            )],
        );
        for width in [10, 24, 60, 80, 120] {
            for height in [5, 6, 10, 24, 40] {
                render_app(&mut app, width, height);
            }
        }
        app.art_cache.insert(uri, None);
        app.key(KeyEvent::new(K::Char('2'), KeyModifiers::NONE));
        app.key(KeyEvent::new(K::Char('l'), KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::Lyrics);
        app.key(KeyEvent::new(K::Char('l'), KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::NowPlaying);
    }

    #[tokio::test]
    async fn now_playing_handles_missing_media_and_small_terminals() {
        let mut app = test_app();
        app.screen = Screen::NowPlaying;
        assert!(screen_text(&render_app(&mut app, 80, 24)).contains("Nothing playing"));
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.art_cache.insert(uri.clone(), None);
        app.lyrics = LyricsState::Failed(uri);
        let text = screen_text(&render_app(&mut app, 120, 40));
        assert!(text.contains("(no album art)"));
        assert!(text.contains("Synced lyrics unavailable"));
        assert!(text.contains("Current song"));
        for width in [10, 24, 60, 80, 120] {
            for height in [5, 6, 10, 24, 40] {
                render_app(&mut app, width, height);
            }
        }
    }

    #[tokio::test]
    async fn synced_lyric_tracks_seeking_pause_and_current_uri() {
        let mut app = test_app();
        app.current = Some(current_track());
        app.state = PlayState::Paused;
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.lyrics = LyricsState::Loaded(
            uri,
            vec![
                (Some(1_000), "First line".into()),
                (Some(2_000), "Second line".into()),
            ],
        );
        assert_eq!(app.current_lyric(), None);
        app.on_event(json!({"event": "seeked", "time_position": 1_500}));
        assert_eq!(app.current_lyric(), Some("First line"));
        app.on_event(json!({"event": "seeked", "time_position": 2_500}));
        assert_eq!(app.current_lyric(), Some("Second line"));
        app.on_event(json!({"event": "seeked", "time_position": 500}));
        assert_eq!(app.current_lyric(), None);
        app.current.as_mut().unwrap().track.uri = "local:track:another-song".into();
        app.set_pos(2_500);
        assert_eq!(app.current_lyric(), None);
    }

    #[tokio::test]
    async fn now_playing_loads_current_media_and_ignores_old_lyric_results() {
        let mut app = test_app();
        app.current = Some(current_track());
        let first = app.current.as_ref().unwrap().track.uri.clone();
        app.key(KeyEvent::new(K::Char('2'), KeyModifiers::NONE));
        assert!(app.art_loading.contains(&first));
        assert!(matches!(&app.lyrics, LyricsState::Fetching(uri) if *uri == first));

        app.on_event(json!({"event": "track_playback_started", "tl_track": {
            "tlid": 2, "track": {"uri": "local:track:next", "name": "Next song"}
        }}));
        assert!(app.art_loading.contains("local:track:next"));
        assert!(matches!(&app.lyrics, LyricsState::Fetching(uri) if uri == "local:track:next"));
        app.on_msg(Msg::Lyrics(
            first,
            Ok(Lyrics {
                synced: Some("[00:00.00] Wrong song".into()),
                plain: None,
            }),
        ));
        assert_eq!(app.current_lyric(), None);
        assert!(matches!(&app.lyrics, LyricsState::Fetching(uri) if uri == "local:track:next"));
        app.on_msg(Msg::Lyrics(
            "local:track:next".into(),
            Ok(Lyrics {
                synced: Some("[00:00.00] Correct song".into()),
                plain: None,
            }),
        ));
        assert_eq!(app.current_lyric(), Some("Correct song"));

        app.on_event(json!({"event": "playback_state_changed", "new_state": "stopped"}));
        assert!(matches!(app.lyrics, LyricsState::Idle));
        assert_eq!(app.current_lyric(), None);
    }

    #[tokio::test]
    async fn failed_lyrics_complete_loading_without_automatic_retry_loops() {
        let mut app = test_app();
        app.screen = Screen::NowPlaying;
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.art_cache.insert(uri.clone(), None);
        app.lyrics = LyricsState::Fetching(uri.clone());
        app.on_msg(Msg::Lyrics(uri.clone(), Err("Network unavailable".into())));
        app.tick();
        assert!(matches!(&app.lyrics, LyricsState::Failed(u) if *u == uri));
        assert!(app
            .message
            .as_ref()
            .unwrap()
            .0
            .contains("Network unavailable"));
    }

    #[tokio::test]
    async fn eight_and_f1_open_the_scrollable_key_bindings_screen() {
        for shortcut in [K::Char('8'), K::F(1)] {
            let mut app = test_app();
            app.screen = Screen::Browser;
            app.key(KeyEvent::new(shortcut, KeyModifiers::NONE));
            assert_eq!(app.screen, Screen::Help);
            assert_eq!(app.prev_screen, Screen::Browser);

            let backend = ratatui::backend::TestBackend::new(100, 30);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let rendered: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(rendered.contains("Help"));
            assert!(rendered.contains("Keys - Movement"));

            app.key(KeyEvent::new(K::Down, KeyModifiers::NONE));
            assert_eq!(app.help_offset, 1);
            app.key(KeyEvent::new(K::PageDown, KeyModifiers::NONE));
            assert_eq!(app.help_offset, 11);
            app.key(KeyEvent::new(K::PageUp, KeyModifiers::NONE));
            assert_eq!(app.help_offset, 1);
            app.key(KeyEvent::new(K::Up, KeyModifiers::NONE));
            assert_eq!(app.help_offset, 0);

            app.key(KeyEvent::new(K::Char('4'), KeyModifiers::NONE));
            assert_eq!(app.screen, Screen::Browser);
        }
    }

    #[tokio::test]
    async fn browser_arrows_and_existing_keys_navigate_folders() {
        for (into, up) in [(K::Right, K::Left), (K::Enter, K::Backspace)] {
            let mut app = test_app();
            app.screen = Screen::Browser;
            app.volume = 50;
            let files = Ref {
                kind: "directory".into(),
                uri: "file:///music".into(),
                name: "Files".into(),
            };
            let album = Ref {
                kind: "directory".into(),
                uri: "file:///music/Album".into(),
                name: "Album".into(),
            };
            app.cache_browse(
                String::new(),
                vec![
                    Ref {
                        kind: "directory".into(),
                        uri: "local:directory".into(),
                        name: "Local media".into(),
                    },
                    files.clone(),
                ],
            );
            app.cache_browse(files.uri.clone(), vec![album.clone()]);
            app.cache_browse(album.uri.clone(), vec![]);
            app.browse_load();

            app.key(KeyEvent::new(K::Down, KeyModifiers::NONE));
            app.key(KeyEvent::new(into, KeyModifiers::NONE));
            assert_eq!(app.browse_path(), "/Files");
            assert_eq!(app.browse_uri().as_deref(), Some(files.uri.as_str()));

            app.key(KeyEvent::new(K::Down, KeyModifiers::NONE));
            app.key(KeyEvent::new(into, KeyModifiers::NONE));
            assert_eq!(app.browse_path(), "/Files/Album");
            assert_eq!(app.browse_uri().as_deref(), Some(album.uri.as_str()));

            app.key(KeyEvent::new(up, KeyModifiers::NONE));
            assert_eq!(app.browse_path(), "/Files");
            assert_eq!(app.browser.cursor, 1);
            assert_eq!(app.browser.cur(), Some(&album));

            app.key(KeyEvent::new(up, KeyModifiers::NONE));
            assert_eq!(app.browse_path(), "/");
            assert_eq!(app.browser.cursor, 1);
            assert_eq!(app.browser.cur(), Some(&files));

            app.key(KeyEvent::new(up, KeyModifiers::NONE));
            assert_eq!(app.browse_stack.len(), 1);
            assert_eq!(app.volume, 50);
        }
    }

    #[tokio::test]
    async fn browser_arrows_handle_nonfolders_and_empty_lists() {
        let mut app = test_app();
        app.screen = Screen::Browser;
        app.volume = 50;
        for kind in ["track", "parent"] {
            let item = Ref {
                kind: kind.into(),
                uri: "file:///music/song.mp3".into(),
                name: "Song".into(),
            };
            app.browser.set(vec![item.clone()]);
            app.key(KeyEvent::new(K::Right, KeyModifiers::NONE));
            assert_eq!(app.browse_stack.len(), 1);
            assert_eq!(app.browser.cur(), Some(&item));
        }

        app.browser.set(vec![]);
        app.key(KeyEvent::new(K::Right, KeyModifiers::NONE));
        app.key(KeyEvent::new(K::Left, KeyModifiers::NONE));
        assert_eq!(app.browse_stack.len(), 1);
        assert!(app.browser.is_empty());

        app.cache_browse(String::new(), vec![]);
        app.browse_stack
            .push((Some("file:///music".into()), "Files".into(), 0));
        app.key(KeyEvent::new(K::Left, KeyModifiers::NONE));
        assert_eq!(app.browse_stack.len(), 1);
        assert!(app.browser.is_empty());
        assert_eq!(app.volume, 50);
    }

    #[tokio::test]
    async fn browser_navigation_preserves_other_screens_and_volume_keys() {
        let mut app = test_app();
        app.volume = 50;
        app.screen = Screen::Browser;
        app.key(KeyEvent::new(K::Char('+'), KeyModifiers::NONE));
        assert_eq!(app.volume, 50 + VOL_STEP);
        app.key(KeyEvent::new(K::Char('-'), KeyModifiers::NONE));
        assert_eq!(app.volume, 50);

        app.screen = Screen::Playlist;
        app.key(KeyEvent::new(K::Right, KeyModifiers::NONE));
        assert_eq!(app.volume, 50 + VOL_STEP);
        app.key(KeyEvent::new(K::Left, KeyModifiers::NONE));
        assert_eq!(app.volume, 50);

        for screen in [Screen::Library, Screen::Editor] {
            app.screen = screen;
            app.key(KeyEvent::new(K::Right, KeyModifiers::NONE));
            assert_eq!(app.lib_col, usize::from(screen == Screen::Library));
            assert_eq!(app.ed_col, usize::from(screen == Screen::Editor));
            app.key(KeyEvent::new(K::Left, KeyModifiers::NONE));
        }
        assert_eq!((app.lib_col, app.ed_col), (0, 0));
        assert_eq!(app.volume, 50);
    }

    #[tokio::test]
    async fn failed_cover_completes_loading_instead_of_getting_stuck() {
        let mut app = test_app();
        let uri = "local:track:song.mp3".to_string();
        app.art_loading.insert(uri.clone());
        app.on_msg(Msg::Cover(uri.clone(), Err("HTTP 404".into())));
        assert!(!app.art_loading.contains(&uri));
        assert!(matches!(app.art_cache.get(&uri), Some(None)));
        assert!(app.message.as_ref().unwrap().0.contains("HTTP 404"));
    }

    #[test]
    fn menu_scrolls_classically_and_skips_inactive() {
        let mut m: Menu<i32> = Menu {
            height: 3,
            ..Default::default()
        };
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

    #[tokio::test]
    async fn browsing_limits_metadata_and_retries_only_after_invalidation() {
        let mut app = test_app();
        app.connected = true;
        app.screen = Screen::Browser;
        app.browser.height = 20;
        app.browser.set(
            (0..10_000)
                .map(|i| Ref {
                    kind: "track".into(),
                    uri: format!("file:{i}"),
                    name: format!("Song {i}"),
                })
                .collect(),
        );
        app.ensure_visible_metadata();
        assert_eq!(app.metadata_pending.len(), 40);
        app.ensure_visible_metadata();
        assert_eq!(app.metadata_batches, 1);
        app.browser.offset = 500;
        app.ensure_visible_metadata();
        app.browser.offset = 1000;
        app.ensure_visible_metadata();
        assert_eq!(app.metadata_batches, 2);
        assert_eq!(app.metadata_pending.len(), 80);
        assert!(!app.metadata_pending.contains("file:1000"));
        let failed: Vec<String> = (0..40).map(|i| format!("file:{i}")).collect();
        app.on_msg(Msg::Metadata(
            failed.clone(),
            app.library_generation,
            Err("timeout".into()),
        ));
        app.browser.offset = 0;
        app.ensure_visible_metadata();
        assert_eq!(
            app.metadata_batches, 1,
            "failed rows must not retry on every draw"
        );
        app.invalidate_library();
        app.ensure_visible_metadata();
        assert_eq!(app.metadata_pending.len(), 40);
        app.on_msg(Msg::Metadata(
            failed,
            app.library_generation - 1,
            Ok(vec![]),
        ));
        assert_eq!(
            app.metadata_pending.len(),
            40,
            "stale completion must not clear new requests"
        );
    }

    #[tokio::test]
    async fn browse_navigation_deduplicates_and_isolates_stale_results() {
        let mut app = test_app();
        app.screen = Screen::Browser;
        app.cache_browse(String::new(), vec![]);
        app.browse_into("a".into(), "A".into());
        app.browse_load();
        assert_eq!(app.browse_pending.len(), 1);
        app.browse_up();
        assert!(!app.browse_loading, "Back to a cached folder is immediate");
        app.browse_into("b".into(), "B".into());
        app.on_msg(Msg::Browsed("a".into(), 0, Ok(vec![])));
        assert!(
            app.browse_loading,
            "another folder must not stop the spinner"
        );
        app.invalidate_library();
        app.on_msg(Msg::Browsed("b".into(), 0, Ok(vec![])));
        assert!(app.browse_loading);
        assert!(app.browse_pending.contains("b"));
        app.on_msg(Msg::Browsed(
            "b".into(),
            app.library_generation,
            Err("failed".into()),
        ));
        assert!(!app.browse_loading);
        assert!(!app.browse_pending.contains("b"));
        assert!(app.message.as_ref().unwrap().0.contains("failed"));
    }

    #[tokio::test]
    async fn render_cache_reuses_pixels_and_invalidates_content_and_geometry() {
        let mut app = test_app();
        app.screen = Screen::NowPlaying;
        app.pixel_size = Some((960, 640));
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.art_cache
            .insert(uri.clone(), Some(image::DynamicImage::new_rgb8(100, 100)));
        app.lyrics = LyricsState::Loaded(uri.clone(), vec![(Some(0), "First line".into())]);
        let first = render_app(&mut app, 120, 40);
        let before = app.fb_blits.borrow().clone();
        assert_eq!(before.len(), 2);
        assert_eq!(first, render_app(&mut app, 120, 40));
        for (a, b) in before.iter().zip(app.fb_blits.borrow().iter()) {
            assert!(Arc::ptr_eq(&a.rgb, &b.rgb));
            assert_eq!(a.area(), b.area());
        }
        app.lyrics = LyricsState::Loaded(uri, vec![(Some(0), "Next line".into())]);
        render_app(&mut app, 120, 40);
        assert!(Arc::ptr_eq(&before[0].rgb, &app.fb_blits.borrow()[0].rgb));
        assert!(!Arc::ptr_eq(&before[1].rgb, &app.fb_blits.borrow()[1].rgb));
        app.on_msg(Msg::Cover("other".into(), Ok(None)));
        render_app(&mut app, 120, 40);
        assert!(!Arc::ptr_eq(&before[0].rgb, &app.fb_blits.borrow()[0].rgb));
        app.pixel_size = Some((1280, 800));
        render_app(&mut app, 160, 50);
        for (a, b) in before.iter().zip(app.fb_blits.borrow().iter()) {
            assert!(!Arc::ptr_eq(&a.rgb, &b.rgb));
            assert_eq!(b.rgb.len(), b.w as usize * b.h as usize * 3);
        }
    }

    /// Repeatable CPU-only rendering workload; run in release mode with --ignored --nocapture.
    #[tokio::test]
    #[ignore = "manual rendering benchmark"]
    async fn rendering_benchmark() {
        let mut app = test_app();
        app.current = Some(current_track());
        let uri = app.current.as_ref().unwrap().track.uri.clone();
        app.art_cache.insert(
            uri.clone(),
            Some(image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(
                600,
                600,
                |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]),
            ))),
        );
        app.lyrics = LyricsState::Loaded(uri, vec![(Some(0), "Anna musiikin soida".into())]);
        app.browser.set(
            (0..10_000)
                .map(|i| Ref {
                    kind: "directory".into(),
                    uri: format!("file:///music/{i}"),
                    name: format!("Album {i}"),
                })
                .collect(),
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        for (label, screen, pixels) in [
            ("browser 10k", Screen::Browser, None),
            ("terminal cover", Screen::NowPlaying, None),
            ("pixel cover + lyric", Screen::NowPlaying, Some((960, 640))),
        ] {
            app.screen = screen;
            app.pixel_size = pixels;
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let start = Instant::now();
            for _ in 0..100 {
                terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
                std::hint::black_box(terminal.backend().buffer());
            }
            println!(
                "{label}: {:.3} ms/frame (100 warm frames)",
                start.elapsed().as_secs_f64() * 10.0
            );
        }
    }
}
