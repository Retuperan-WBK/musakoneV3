//! Application state, key handling (ncmpcpp bindings) and async actions.
use crate::api::{Api, Lyrics};
use crate::model::*;
use crate::rpc::Rpc;
use anyhow::Result;
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Help,
    Queue,
    Browser,
    Search,
    Mix,
    Playlists,
    Lyrics,
}

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Help => "Help",
            View::Queue => "Playlist",
            View::Browser => "Browse",
            View::Search => "Search engine",
            View::Mix => "Mix",
            View::Playlists => "Playlist editor",
            View::Lyrics => "Lyrics",
        }
    }
    const TABS: [View; 6] = [
        View::Help,
        View::Queue,
        View::Browser,
        View::Search,
        View::Mix,
        View::Playlists,
    ];
}

#[derive(Default, Clone, Copy)]
pub struct ListState {
    pub cursor: usize,
    pub height: usize,
}

impl ListState {
    fn mv(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
    }
    fn clamp(&mut self, len: usize) {
        self.cursor = self.cursor.min(len.saturating_sub(1));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

pub enum SearchRow {
    Header(String),
    Track(Track),
    Ref(Ref),
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
    Searched(Vec<SearchRow>),
    Playlists(Vec<Playlist>),
    PlaylistTracks(i64, Vec<String>),
    Recs(Vec<Recommendation>),
    Autoplay(AutoplayStatus),
    LoggedIn(String),
    Lyrics(String, Lyrics),
    Info(String),
    Error(String),
    ReloadPlaylists,
    Nothing,
}

pub enum PromptKind {
    Username,
    Password(String),
    Search,
    Find,
    NewPlaylist,
    RenamePlaylist(Playlist),
    SaveQueue,
    ConfirmClear,
    ConfirmDeletePlaylist(Playlist),
    Jump,
}

pub struct Prompt {
    pub label: String,
    pub buf: String,
    pub masked: bool,
    pub confirm: bool,
    pub kind: PromptKind,
}

pub struct Popup {
    pub title: String,
    pub items: Vec<(i64, String)>,
    pub cursor: usize,
    pub uris: Vec<String>,
}

pub struct App {
    rpc: Arc<Rpc>,
    api: Arc<Api>,
    tx: mpsc::UnboundedSender<Msg>,
    pub token_path: PathBuf,
    pub lyrics_dir: PathBuf,

    pub view: View,
    pub quit: bool,
    pub connected: bool,
    pub mopidy_ok: bool,
    pub user: Option<String>,

    pub state: PlayState,
    pub current: Option<TlTrack>,
    pos_ms: u64,
    pos_at: Instant,
    pub volume: i64,
    pub repeat: bool,
    pub random: bool,
    pub single: bool,
    pub consume: bool,
    ticks: u64,

    pub queue: Vec<TlTrack>,
    pub queue_list: ListState,

    pub tracks: HashMap<String, Track>,
    pub browse_stack: Vec<(Option<String>, String, usize)>,
    pub browse_items: Vec<Ref>,
    pub browse_list: ListState,
    browse_cache: HashMap<String, Vec<Ref>>,
    pub browse_loading: bool,

    pub search_query: String,
    pub search_rows: Vec<SearchRow>,
    pub search_list: ListState,
    pub searching: bool,

    pub playlists: Vec<Playlist>,
    pub pl_list: ListState,
    pub pl_tracks: Vec<String>,
    pub plt_list: ListState,
    pub pl_right: bool,
    pl_cache: HashMap<i64, Vec<String>>,

    pub recs: Vec<Recommendation>,
    pub rec_list: ListState,
    pub autoplay: Option<AutoplayStatus>,
    pub recs_loading: bool,

    pub lyrics: Option<(String, Vec<(Option<u64>, String)>)>,
    pub lyrics_list: ListState,
    pub lyrics_follow: bool,
    lyrics_loading: Option<String>,
    pub help_list: ListState,

    pub prompt: Option<Prompt>,
    pub popup: Option<Popup>,
    pub message: Option<(String, Instant, bool)>,
    find: String,
}

const VOL_STEP: i64 = 2;
const SEEK_MS: i64 = 5000;

impl App {
    pub fn new(
        rpc: Arc<Rpc>,
        api: Arc<Api>,
        tx: mpsc::UnboundedSender<Msg>,
        token_path: PathBuf,
        lyrics_dir: PathBuf,
        has_token: bool,
    ) -> App {
        let mut app = App {
            rpc,
            api,
            tx,
            token_path,
            lyrics_dir,
            view: View::Queue,
            quit: false,
            connected: false,
            mopidy_ok: true,
            user: None,
            state: PlayState::Stopped,
            current: None,
            pos_ms: 0,
            pos_at: Instant::now(),
            volume: 0,
            repeat: false,
            random: false,
            single: false,
            consume: false,
            ticks: 0,
            queue: vec![],
            queue_list: ListState::default(),
            tracks: HashMap::new(),
            browse_stack: vec![(None, String::new(), 0)],
            browse_items: vec![],
            browse_list: ListState::default(),
            browse_cache: HashMap::new(),
            browse_loading: false,
            search_query: String::new(),
            search_rows: vec![],
            search_list: ListState::default(),
            searching: false,
            playlists: vec![],
            pl_list: ListState::default(),
            pl_tracks: vec![],
            plt_list: ListState::default(),
            pl_right: false,
            pl_cache: HashMap::new(),
            recs: vec![],
            rec_list: ListState::default(),
            autoplay: None,
            recs_loading: false,
            lyrics: None,
            lyrics_list: ListState::default(),
            lyrics_follow: true,
            lyrics_loading: None,
            help_list: ListState::default(),
            prompt: None,
            popup: None,
            message: None,
            find: String::new(),
        };
        if has_token {
            app.check_login();
        } else {
            app.ask_login();
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

    pub fn info(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), Instant::now(), false));
    }
    pub fn error(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), Instant::now(), true));
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

    pub fn track_of<'a>(&'a self, uri: &str) -> Option<&'a Track> {
        self.tracks.get(uri)
    }

    pub fn current_tlid(&self) -> Option<i64> {
        self.current.as_ref().map(|t| t.tlid)
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

    // ---------- login ----------

    fn ask_login(&mut self) {
        self.prompt = Some(Prompt {
            label: "Username: ".into(),
            buf: String::new(),
            masked: false,
            confirm: false,
            kind: PromptKind::Username,
        });
    }

    fn check_login(&self) {
        let api = self.api.clone();
        self.task(async move {
            match api.me().await {
                Ok(v) => Ok(Msg::Info(format!(
                    "Logged in (user {})",
                    v.get("id").and_then(Value::as_i64).unwrap_or(0)
                ))),
                Err(e) if e.to_string().starts_with("unauthorized") => {
                    Ok(Msg::LoggedIn(String::new()))
                }
                Err(e) => Err(e),
            }
        });
    }

    // ---------- mopidy state ----------

    pub fn refresh_all(&mut self) {
        let rpc = self.rpc.clone();
        let tx = self.tx.clone();
        self.task(async move {
            let s = rpc.call("core.playback.get_state", json!({})).await?;
            let _ = tx.send(Msg::State(s.as_str().unwrap_or("stopped").into()));
            let c = rpc
                .call("core.playback.get_current_tl_track", json!({}))
                .await?;
            let _ = tx.send(Msg::Current(TlTrack::from_value(&c)));
            let p = rpc
                .call("core.playback.get_time_position", json!({}))
                .await?;
            let _ = tx.send(Msg::Position(p.as_u64().unwrap_or(0)));
            let v = rpc.call("core.mixer.get_volume", json!({})).await?;
            let _ = tx.send(Msg::Volume(v.as_i64().unwrap_or(0)));
            let q = rpc.call("core.tracklist.get_tl_tracks", json!({})).await?;
            let _ = tx.send(Msg::Queue(TlTrack::list(&q)));
            Ok(Msg::Nothing)
        });
        self.load_options();
        self.load_playlists();
        self.load_autoplay();
        if self.browse_items.is_empty() {
            self.browse_load();
        }
    }

    fn load_options(&self) {
        let rpc = self.rpc.clone();
        self.task(async move {
            let mut o = [false; 4];
            for (i, m) in ["get_repeat", "get_random", "get_single", "get_consume"]
                .iter()
                .enumerate()
            {
                o[i] = rpc
                    .call(&format!("core.tracklist.{m}"), json!({}))
                    .await?
                    .as_bool()
                    .unwrap_or(false);
            }
            Ok(Msg::Options(o[0], o[1], o[2], o[3]))
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

    fn rpc_fire(&self, method: &'static str, params: Value) {
        let rpc = self.rpc.clone();
        self.task(async move {
            rpc.call(method, params).await?;
            Ok(Msg::Nothing)
        });
    }

    fn add_uris(&mut self, uris: Vec<String>, play: bool) {
        if uris.is_empty() {
            self.error("Nothing to add");
            return;
        }
        let n = uris.len();
        let rpc = self.rpc.clone();
        self.task(async move {
            let added = rpc
                .call("core.tracklist.add", json!({"uris": uris}))
                .await?;
            let tl = TlTrack::list(&added);
            if play {
                if let Some(first) = tl.first() {
                    rpc.call("core.playback.play", json!({"tlid": first.tlid}))
                        .await?;
                }
            }
            Ok(Msg::Info(format!(
                "Added {n} track{}",
                if n == 1 { "" } else { "s" }
            )))
        });
    }

    /// Resolve a non-track ref (album, artist, directory, playlist) to track uris, then add.
    fn add_ref(&mut self, r: Ref, play: bool) {
        if r.is_track() {
            self.add_uris(vec![r.uri], play);
            return;
        }
        let rpc = self.rpc.clone();
        let tx = self.tx.clone();
        self.task(async move {
            let mut uris: Vec<String> = flatten_lookup(
                &rpc.call("core.library.lookup", json!({"uris": [r.uri]}))
                    .await?,
            )
            .into_iter()
            .map(|t| t.uri)
            .collect();
            if uris.is_empty() {
                uris = browse_tracks(&rpc, &r.uri, 0).await?;
            }
            if uris.is_empty() {
                return Ok(Msg::Error(format!("No tracks in {}", r.name)));
            }
            let n = uris.len();
            let added = rpc
                .call("core.tracklist.add", json!({"uris": uris}))
                .await?;
            if play {
                if let Some(first) = TlTrack::list(&added).first() {
                    rpc.call("core.playback.play", json!({"tlid": first.tlid}))
                        .await?;
                }
            }
            let _ = tx.send(Msg::Info(format!("Added {n} tracks from {}", r.name)));
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
            self.browse_items = items.clone();
            self.browse_list.clamp(self.browse_rows());
            let uris: Vec<String> = items
                .iter()
                .filter(|r| r.is_track())
                .map(|r| r.uri.clone())
                .collect();
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

    pub fn browse_rows(&self) -> usize {
        self.browse_items.len() + usize::from(self.browse_stack.len() > 1)
    }

    /// Item under cursor, None for the ".." row.
    fn browse_sel(&self) -> Option<&Ref> {
        let off = usize::from(self.browse_stack.len() > 1);
        self.browse_list
            .cursor
            .checked_sub(off)
            .and_then(|i| self.browse_items.get(i))
    }

    pub fn browse_into(&mut self, uri: String, name: String) {
        if let Some(top) = self.browse_stack.last_mut() {
            top.2 = self.browse_list.cursor;
        }
        self.browse_stack.push((Some(uri), name, 0));
        self.browse_list.cursor = 0;
        self.browse_items.clear();
        self.browse_load();
    }

    fn browse_up(&mut self) {
        if self.browse_stack.len() > 1 {
            self.browse_stack.pop();
            self.browse_list.cursor = self.browse_stack.last().map(|t| t.2).unwrap_or(0);
            self.browse_items.clear();
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

    // ---------- search ----------

    fn do_search(&mut self, q: String) {
        if q.trim().is_empty() {
            return;
        }
        self.search_query = q.clone();
        self.searching = true;
        let rpc = self.rpc.clone();
        self.task(async move {
            let v = rpc
                .call(
                    "core.library.search",
                    json!({"query": {"any": [q]}, "exact": false}),
                )
                .await?;
            let mut tracks: Vec<Track> = vec![];
            let mut albums: Vec<Ref> = vec![];
            let mut artists: Vec<Ref> = vec![];
            let mut seen = std::collections::HashSet::new();
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
                        .get("artists")
                        .and_then(Value::as_array)
                        .and_then(|x| x.first())
                        .and_then(|x| x.get("name"))
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
            }
            let mut rows = vec![];
            if !tracks.is_empty() {
                rows.push(SearchRow::Header(format!("Tracks ({})", tracks.len())));
                rows.extend(tracks.into_iter().map(SearchRow::Track));
            }
            if !albums.is_empty() {
                rows.push(SearchRow::Header(format!("Albums ({})", albums.len())));
                rows.extend(albums.into_iter().map(SearchRow::Ref));
            }
            if !artists.is_empty() {
                rows.push(SearchRow::Header(format!("Artists ({})", artists.len())));
                rows.extend(artists.into_iter().map(SearchRow::Ref));
            }
            Ok(Msg::Searched(rows))
        });
    }

    // ---------- playlists (Gleam backend) ----------

    fn load_playlists(&self) {
        let api = self.api.clone();
        self.task(async move { Ok(Msg::Playlists(api.playlists().await?)) });
    }

    pub fn sel_playlist(&self) -> Option<&Playlist> {
        self.playlists.get(self.pl_list.cursor)
    }

    fn load_pl_tracks(&mut self, force: bool) {
        let Some(p) = self.sel_playlist().cloned() else {
            self.pl_tracks.clear();
            return;
        };
        if !force {
            if let Some(t) = self.pl_cache.get(&p.id) {
                self.pl_tracks = t.clone();
                self.plt_list.clamp(self.pl_tracks.len());
                let uris = self.pl_tracks.clone();
                self.lookup_missing(&uris);
                return;
            }
        }
        let api = self.api.clone();
        self.task(async move { Ok(Msg::PlaylistTracks(p.id, api.playlist_tracks(p.id).await?)) });
    }

    fn add_to_playlist(&mut self, pid: i64, uris: Vec<String>) {
        let api = self.api.clone();
        let n = uris.len();
        let tx = self.tx.clone();
        self.pl_cache.remove(&pid);
        self.task(async move {
            for u in uris {
                api.add_track(pid, &u).await?;
            }
            let _ = tx.send(Msg::ReloadPlaylists);
            Ok(Msg::Info(format!(
                "Added {n} track{} to playlist",
                if n == 1 { "" } else { "s" }
            )))
        });
    }

    fn open_add_popup(&mut self, uris: Vec<String>) {
        if uris.is_empty() {
            return;
        }
        let mine: Vec<(i64, String)> = self
            .playlists
            .iter()
            .filter(|p| self.user_id().is_none_or(|me| p.user_id == me))
            .map(|p| (p.id, p.name.clone()))
            .collect();
        if mine.is_empty() {
            self.error("No playlists yet (S to save queue, N for new)");
            return;
        }
        self.popup = Some(Popup {
            title: "Add to playlist".into(),
            items: mine,
            cursor: 0,
            uris,
        });
    }

    fn user_id(&self) -> Option<i64> {
        self.user.as_ref().and_then(|u| u.parse().ok())
    }

    // ---------- mix ----------

    fn load_autoplay(&self) {
        let api = self.api.clone();
        self.task(async move { Ok(Msg::Autoplay(api.autoplay().await?)) });
    }

    fn load_recs(&mut self) {
        self.recs_loading = true;
        let api = self.api.clone();
        let scope = self
            .autoplay
            .as_ref()
            .map(|a| {
                if a.settings.mode == "room" {
                    "room"
                } else {
                    "me"
                }
            })
            .unwrap_or("room");
        self.task(async move { Ok(Msg::Recs(api.recommendations(scope).await?)) });
    }

    // ---------- lyrics ----------

    fn ensure_lyrics(&mut self) {
        let Some(cur) = self.current.as_ref().map(|c| c.track.clone()) else {
            return;
        };
        if self.lyrics.as_ref().is_some_and(|(u, _)| *u == cur.uri)
            || self.lyrics_loading.as_deref() == Some(&cur.uri)
        {
            return;
        }
        self.lyrics_loading = Some(cur.uri.clone());
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
        let (_, lines) = self.lyrics.as_ref()?;
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
        if self.state == PlayState::Playing && self.ticks.is_multiple_of(10) && self.connected {
            self.sync_position();
        }
        if let Some((_, at, _)) = &self.message {
            if at.elapsed().as_secs() >= 4 {
                self.message = None;
            }
        }
        if self.view == View::Lyrics {
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
            self.error(format!("Backend: {err}"));
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
            "track_playback_ended" => {}
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
                self.error("Backend lost Mopidy");
            }
            "autoplay_added" => {
                let n = v.get("count").and_then(Value::as_i64).unwrap_or(0);
                self.info(format!("Autoplay added {n} tracks"));
            }
            _ => {}
        }
    }

    pub fn on_msg(&mut self, m: Msg) {
        match m {
            Msg::Nothing => {}
            Msg::Info(s) => self.info(s),
            Msg::Error(e) => {
                if e.starts_with("unauthorized") && self.user.is_none() {
                    self.error("Log in first (L)")
                } else {
                    self.error(e)
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
                self.queue = q;
                self.queue_list.clamp(self.queue.len());
            }
            Msg::Options(r, z, y, c) => {
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
            Msg::Tracks(ts) => self.remember(ts),
            Msg::Searched(rows) => {
                self.searching = false;
                self.remember(rows.iter().filter_map(|r| match r {
                    SearchRow::Track(t) => Some(t.clone()),
                    _ => None,
                }));
                self.search_rows = rows;
                self.search_list.cursor = if self.search_rows.len() > 1 { 2 } else { 0 };
            }
            Msg::Playlists(p) => {
                self.playlists = p;
                self.pl_list.clamp(self.playlists.len());
                self.load_pl_tracks(false);
            }
            Msg::PlaylistTracks(id, uris) => {
                self.pl_cache.insert(id, uris);
                if self.sel_playlist().map(|p| p.id) == Some(id) {
                    self.load_pl_tracks(false);
                }
            }
            Msg::ReloadPlaylists => {
                self.pl_cache.clear();
                self.load_playlists();
            }
            Msg::Recs(r) => {
                self.recs_loading = false;
                self.recs = r;
                self.rec_list.clamp(self.recs.len());
            }
            Msg::Autoplay(a) => self.autoplay = Some(a),
            Msg::LoggedIn(token) => {
                if token.is_empty() {
                    let _ = std::fs::remove_file(&self.token_path);
                    self.api.set_token(None);
                    self.ask_login();
                    return;
                }
                if let Some(parent) = self.token_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&self.token_path, &token);
                self.api.set_token(Some(token.clone()));
                self.rpc.set_token(Some(token.clone()));
                self.user = jwt_sub(&token);
                self.info("Logged in");
                self.load_playlists();
            }
            Msg::Lyrics(uri, l) => {
                if self.lyrics_loading.as_deref() == Some(&uri) {
                    self.lyrics_loading = None;
                }
                let lines = l.lines();
                if lines.is_empty() {
                    self.lyrics = Some((uri, vec![(None, "No lyrics found".into())]));
                } else {
                    self.lyrics = Some((uri, lines));
                }
                self.lyrics_list.cursor = 0;
            }
        }
    }

    // ---------- keys ----------

    pub fn key(&mut self, k: KeyEvent) {
        if self.prompt.is_some() {
            self.prompt_key(k);
            return;
        }
        if self.popup.is_some() {
            self.popup_key(k);
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            K::Char('d') if ctrl => self.page(1),
            K::Char('u') if ctrl => self.page(-1),
            K::Char('q') => self.quit = true,
            K::Char('1') | K::F(1) => self.view = View::Help,
            K::Char('2') => self.view = View::Queue,
            K::Char('3') => self.view = View::Browser,
            K::Char('4') => self.view = View::Search,
            K::Char('5') => self.view = View::Mix,
            K::Char('6') => self.view = View::Playlists,
            K::Char('l') => {
                self.view = if self.view == View::Lyrics {
                    View::Queue
                } else {
                    View::Lyrics
                };
                self.lyrics_follow = true;
            }
            K::Tab => self.cycle(1),
            K::BackTab => self.cycle(-1),

            K::Char('p') | K::Char('P') => match self.state {
                PlayState::Playing => self.rpc_fire("core.playback.pause", json!({})),
                PlayState::Paused => self.rpc_fire("core.playback.resume", json!({})),
                PlayState::Stopped => self.rpc_fire("core.playback.play", json!({})),
            },
            K::Char('s') => self.rpc_fire("core.playback.stop", json!({})),
            K::Char('>') => self.rpc_fire("core.playback.next", json!({})),
            K::Char('<') => self.rpc_fire("core.playback.previous", json!({})),
            K::Char('f') => self.seek(SEEK_MS),
            K::Char('b') => self.seek(-SEEK_MS),
            K::Right | K::Char('+') | K::Char('=') => self.set_volume(self.volume + VOL_STEP),
            K::Left | K::Char('-') => self.set_volume(self.volume - VOL_STEP),
            K::Char('r') => {
                self.rpc_fire("core.tracklist.set_repeat", json!({"value": !self.repeat}))
            }
            K::Char('z') => {
                self.rpc_fire("core.tracklist.set_random", json!({"value": !self.random}))
            }
            K::Char('y') => {
                self.rpc_fire("core.tracklist.set_single", json!({"value": !self.single}))
            }
            K::Char('R') => self.rpc_fire(
                "core.tracklist.set_consume",
                json!({"value": !self.consume}),
            ),
            K::Char('Z') => self.rpc_fire("core.tracklist.shuffle", json!({})),
            K::Char('c') => self.ask(PromptKind::ConfirmClear, "Clear playlist? [y/n] "),
            K::Char('C') => {
                let keep = self.current_tlid();
                let rm: Vec<i64> = self
                    .queue
                    .iter()
                    .map(|t| t.tlid)
                    .filter(|t| Some(*t) != keep)
                    .collect();
                if !rm.is_empty() {
                    self.rpc_fire("core.tracklist.remove", json!({"criteria": {"tlid": rm}}));
                }
            }
            K::Char('o') => {
                if let Some(cur) = self.current_tlid() {
                    if let Some(i) = self.queue.iter().position(|t| t.tlid == cur) {
                        self.queue_list.cursor = i;
                    }
                    if self.view == View::Lyrics {
                        self.lyrics_follow = true;
                    } else {
                        self.view = View::Queue;
                    }
                }
            }
            K::Char('/') => self.ask(PromptKind::Find, "Find: "),
            K::Char('.') => self.find_next(1),
            K::Char(',') => self.find_next(-1),
            K::Char('g') => self.ask(
                PromptKind::Jump,
                "Position to go (in %, mm:ss, m:ss, or s): ",
            ),
            K::Char('u') => self.reload_view(),
            K::Char('A') => self.toggle_autoplay(),
            K::Char('F') => {
                let api = self.api.clone();
                self.info("Asking autoplay for a batch…");
                self.task(async move {
                    Ok(Msg::Info(format!(
                        "Autoplay added {} tracks",
                        api.autoplay_fill().await?
                    )))
                });
            }
            K::Char('S') => self.ask(PromptKind::SaveQueue, "Save playlist as: "),
            K::Char('N') => self.ask(PromptKind::NewPlaylist, "New playlist name: "),
            K::Char('L') => {
                let _ = std::fs::remove_file(&self.token_path);
                self.api.set_token(None);
                self.rpc.set_token(None);
                self.user = None;
                self.ask_login();
            }
            _ => self.view_key(k),
        }
    }

    fn cycle(&mut self, d: isize) {
        let i = View::TABS.iter().position(|v| *v == self.view).unwrap_or(1) as isize;
        let n = View::TABS.len() as isize;
        self.view = View::TABS[((i + d).rem_euclid(n)) as usize];
    }

    fn ask(&mut self, kind: PromptKind, label: &str) {
        let confirm = matches!(
            kind,
            PromptKind::ConfirmClear | PromptKind::ConfirmDeletePlaylist(_)
        );
        let masked = matches!(kind, PromptKind::Password(_));
        let buf = match &kind {
            PromptKind::Search => self.search_query.clone(),
            PromptKind::RenamePlaylist(p) => p.name.clone(),
            _ => String::new(),
        };
        self.prompt = Some(Prompt {
            label: label.into(),
            buf,
            masked,
            confirm,
            kind,
        });
    }

    fn seek(&mut self, delta: i64) {
        let Some(len) = self.current.as_ref().and_then(|c| c.track.length) else {
            return;
        };
        let pos = (self.elapsed_ms() as i64 + delta).clamp(0, len as i64 - 1) as u64;
        self.set_pos(pos);
        self.rpc_fire("core.playback.seek", json!({"time_position": pos}));
    }

    fn set_volume(&mut self, v: i64) {
        let v = v.clamp(0, 100);
        self.volume = v;
        self.rpc_fire("core.mixer.set_volume", json!({"volume": v}));
    }

    fn toggle_autoplay(&mut self) {
        let on = self
            .autoplay
            .as_ref()
            .map(|a| a.settings.enabled)
            .unwrap_or(false);
        let api = self.api.clone();
        self.task(async move {
            Ok(Msg::Autoplay(
                api.set_autoplay(json!({"enabled": !on})).await?,
            ))
        });
        self.info(if on { "Autoplay off" } else { "Autoplay on" });
    }

    fn reload_view(&mut self) {
        match self.view {
            View::Browser => {
                self.browse_cache
                    .remove(&self.browse_uri().unwrap_or_default());
                self.browse_load();
            }
            View::Playlists => {
                self.pl_cache.clear();
                self.load_playlists();
            }
            View::Mix => {
                self.load_autoplay();
                self.load_recs();
            }
            View::Lyrics => {
                if let Some(c) = &self.current {
                    let key = c.track.uri.replace(['/', ':'], "_");
                    let _ = std::fs::remove_file(self.lyrics_dir.join(format!("{key}.json")));
                }
                self.lyrics = None;
                self.lyrics_loading = None;
                self.ensure_lyrics();
            }
            _ => self.refresh_all(),
        }
    }

    /// (list, len) of the list the cursor lives in for the current view.
    fn cur_list(&mut self) -> (&mut ListState, usize) {
        match self.view {
            View::Queue => (&mut self.queue_list, self.queue.len()),
            View::Browser => {
                let n = self.browse_rows();
                (&mut self.browse_list, n)
            }
            View::Search => (&mut self.search_list, self.search_rows.len() + 1),
            View::Mix => (&mut self.rec_list, self.recs.len()),
            View::Playlists => {
                if self.pl_right {
                    (&mut self.plt_list, self.pl_tracks.len())
                } else {
                    (&mut self.pl_list, self.playlists.len())
                }
            }
            View::Lyrics => {
                let n = self.lyrics.as_ref().map(|l| l.1.len()).unwrap_or(0);
                (&mut self.lyrics_list, n)
            }
            View::Help => (&mut self.help_list, crate::ui::HELP.len()),
        }
    }

    fn mv(&mut self, d: isize) {
        let (l, n) = self.cur_list();
        l.mv(d, n);
        self.after_move();
    }

    fn page(&mut self, d: isize) {
        let (l, n) = self.cur_list();
        let h = l.height.max(1) as isize;
        l.mv(d * h, n);
        self.after_move();
    }

    fn after_move(&mut self) {
        match self.view {
            View::Playlists if !self.pl_right => {
                self.plt_list.cursor = 0;
                self.load_pl_tracks(false);
            }
            View::Lyrics => self.lyrics_follow = false,
            _ => {}
        }
    }

    /// Text of row i in the current view, for `/` find.
    fn row_text(&self, i: usize) -> Option<String> {
        Some(match self.view {
            View::Queue => self.queue.get(i)?.track.title(),
            View::Browser => {
                let off = usize::from(self.browse_stack.len() > 1);
                if i < off {
                    "..".into()
                } else {
                    let r = self.browse_items.get(i - off)?;
                    self.track_of(&r.uri)
                        .map(|t| t.title())
                        .unwrap_or(r.name.clone())
                }
            }
            View::Search => match self.search_rows.get(i.checked_sub(1)?)? {
                SearchRow::Header(h) => h.clone(),
                SearchRow::Track(t) => t.title(),
                SearchRow::Ref(r) => r.name.clone(),
            },
            View::Mix => {
                let r = self.recs.get(i)?;
                format!("{} - {}", r.artist, r.name)
            }
            View::Playlists => {
                if self.pl_right {
                    let u = self.pl_tracks.get(i)?;
                    self.track_of(u).map(|t| t.title()).unwrap_or(u.clone())
                } else {
                    self.playlists.get(i)?.name.clone()
                }
            }
            View::Lyrics => self.lyrics.as_ref()?.1.get(i)?.1.clone(),
            View::Help => crate::ui::HELP.get(i)?.to_string(),
        })
    }

    fn find_next(&mut self, dir: isize) {
        if self.find.is_empty() {
            return;
        }
        let needle = self.find.to_lowercase();
        let (l, n) = self.cur_list();
        let (start, n) = (l.cursor, n);
        if n == 0 {
            return;
        }
        for step in 1..=n {
            let i = (start as isize + dir * step as isize).rem_euclid(n as isize) as usize;
            if self
                .row_text(i)
                .is_some_and(|t| t.to_lowercase().contains(&needle))
            {
                self.cur_list().0.cursor = i;
                self.after_move();
                return;
            }
        }
        self.error(format!("Not found: {}", self.find));
    }

    fn view_key(&mut self, k: KeyEvent) {
        match k.code {
            K::Down | K::Char('j') => return self.mv(1),
            K::Up | K::Char('k') => return self.mv(-1),
            K::PageDown => return self.page(1),
            K::PageUp => return self.page(-1),
            K::Home => {
                self.cur_list().0.cursor = 0;
                return self.after_move();
            }
            K::End => {
                let (l, n) = self.cur_list();
                l.cursor = n.saturating_sub(1);
                return self.after_move();
            }
            K::Char('G') => {
                let (l, n) = self.cur_list();
                l.cursor = n.saturating_sub(1);
                return self.after_move();
            }
            _ => {}
        }
        match self.view {
            View::Queue => self.queue_key(k),
            View::Browser => self.browser_key(k),
            View::Search => self.search_key(k),
            View::Mix => self.mix_key(k),
            View::Playlists => self.playlists_key(k),
            View::Lyrics | View::Help => {}
        }
    }

    fn queue_key(&mut self, k: KeyEvent) {
        let i = self.queue_list.cursor;
        let Some(t) = self.queue.get(i).cloned() else {
            return;
        };
        match k.code {
            K::Enter => self.rpc_fire("core.playback.play", json!({"tlid": t.tlid})),
            K::Char('d') | K::Delete => self.rpc_fire(
                "core.tracklist.remove",
                json!({"criteria": {"tlid": [t.tlid]}}),
            ),
            K::Char('m') if i > 0 => {
                self.queue_list.cursor -= 1;
                self.rpc_fire(
                    "core.tracklist.move",
                    json!({"start": i, "end": i + 1, "to_position": i - 1}),
                );
            }
            K::Char('n') if i + 1 < self.queue.len() => {
                self.queue_list.cursor += 1;
                self.rpc_fire(
                    "core.tracklist.move",
                    json!({"start": i, "end": i + 1, "to_position": i + 1}),
                );
            }
            K::Char('a') => self.open_add_popup(vec![t.track.uri]),
            _ => {}
        }
    }

    fn browser_key(&mut self, k: KeyEvent) {
        match k.code {
            K::Backspace | K::Char('h') => return self.browse_up(),
            _ => {}
        }
        let Some(r) = self.browse_sel().cloned() else {
            if matches!(k.code, K::Enter) {
                self.browse_up();
            }
            return;
        };
        match k.code {
            K::Enter => {
                if r.is_track() {
                    self.add_uris(vec![r.uri], true);
                } else {
                    self.browse_into(r.uri, r.name);
                }
            }
            K::Char(' ') => self.add_ref(r, false),
            K::Char('a') if r.is_track() => self.open_add_popup(vec![r.uri]),
            _ => {}
        }
    }

    fn search_key(&mut self, k: KeyEvent) {
        let i = self.search_list.cursor;
        if i == 0 {
            if matches!(k.code, K::Enter | K::Char(' ')) {
                self.ask(PromptKind::Search, "Search: ");
            }
            return;
        }
        let Some(row) = self.search_rows.get(i - 1) else {
            return;
        };
        match (row, k.code) {
            (SearchRow::Track(t), K::Enter) => self.add_uris(vec![t.uri.clone()], true),
            (SearchRow::Track(t), K::Char(' ')) => self.add_uris(vec![t.uri.clone()], false),
            (SearchRow::Track(t), K::Char('a')) => self.open_add_popup(vec![t.uri.clone()]),
            (SearchRow::Ref(r), K::Enter) => {
                let r = r.clone();
                self.view = View::Browser;
                self.browse_into(r.uri, r.name);
            }
            (SearchRow::Ref(r), K::Char(' ')) => self.add_ref(r.clone(), false),
            _ => {}
        }
    }

    fn mix_key(&mut self, k: KeyEvent) {
        if self.recs.is_empty() && !self.recs_loading && matches!(k.code, K::Enter | K::Char(' ')) {
            return self.load_recs();
        }
        let Some(r) = self.recs.get(self.rec_list.cursor).cloned() else {
            return;
        };
        match k.code {
            K::Enter => self.add_uris(vec![r.uri], true),
            K::Char(' ') => self.add_uris(vec![r.uri], false),
            K::Char('a') => self.open_add_popup(vec![r.uri]),
            _ => {}
        }
    }

    fn playlists_key(&mut self, k: KeyEvent) {
        match k.code {
            K::Char('h') | K::Left => {
                self.pl_right = false;
                return;
            }
            K::Char('l') | K::Right => {
                if !self.pl_tracks.is_empty() {
                    self.pl_right = true;
                }
                return;
            }
            _ => {}
        }
        let Some(p) = self.sel_playlist().cloned() else {
            return;
        };
        if !self.pl_right {
            match k.code {
                K::Enter => self.add_uris(self.pl_tracks.clone(), true),
                K::Char(' ') => self.add_uris(self.pl_tracks.clone(), false),
                K::Char('d') | K::Delete => self.ask(
                    PromptKind::ConfirmDeletePlaylist(p.clone()),
                    &format!("Delete playlist \"{}\"? [y/n] ", p.name),
                ),
                K::Char('e') => self.ask(PromptKind::RenamePlaylist(p), "Rename playlist: "),
                _ => {}
            }
            return;
        }
        let i = self.plt_list.cursor;
        let Some(uri) = self.pl_tracks.get(i).cloned() else {
            return;
        };
        match k.code {
            K::Enter => self.add_uris(vec![uri], true),
            K::Char(' ') => self.add_uris(vec![uri], false),
            K::Char('a') => self.open_add_popup(vec![uri]),
            K::Char('d') | K::Delete => {
                self.pl_tracks.remove(i);
                self.pl_cache.insert(p.id, self.pl_tracks.clone());
                self.plt_list.clamp(self.pl_tracks.len());
                let api = self.api.clone();
                self.task(async move {
                    api.remove_track(p.id, &uri).await?;
                    Ok(Msg::Nothing)
                });
            }
            K::Char('m') | K::Char('n') => {
                let to = if k.code == K::Char('m') {
                    i.checked_sub(1)
                } else {
                    (i + 1 < self.pl_tracks.len()).then_some(i + 1)
                };
                let Some(to) = to else { return };
                self.pl_tracks.swap(i, to);
                self.plt_list.cursor = to;
                self.pl_cache.insert(p.id, self.pl_tracks.clone());
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
        if p.confirm {
            let yes = matches!(k.code, K::Char('y') | K::Char('Y'));
            let kind = self.prompt.take().unwrap().kind;
            if yes {
                match kind {
                    PromptKind::ConfirmClear => self.rpc_fire("core.tracklist.clear", json!({})),
                    PromptKind::ConfirmDeletePlaylist(p) => {
                        let api = self.api.clone();
                        self.task(async move {
                            api.delete_playlist(p.id).await?;
                            Ok(Msg::ReloadPlaylists)
                        });
                    }
                    _ => {}
                }
            }
            return;
        }
        match k.code {
            K::Esc => {
                let was_login = matches!(p.kind, PromptKind::Username | PromptKind::Password(_));
                self.prompt = None;
                if was_login {
                    self.info(
                        "Not logged in: plays are anonymous, playlists unavailable (L to log in)",
                    );
                }
            }
            K::Enter => {
                let p = self.prompt.take().unwrap();
                self.submit(p.kind, p.buf);
            }
            K::Backspace => {
                p.buf.pop();
            }
            K::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => p.buf.clear(),
            K::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => p.buf.push(c),
            _ => {}
        }
    }

    fn submit(&mut self, kind: PromptKind, buf: String) {
        match kind {
            PromptKind::Username => {
                if buf.is_empty() {
                    return self.ask_login();
                }
                self.ask(PromptKind::Password(buf), "Password: ");
            }
            PromptKind::Password(user) => {
                let api = self.api.clone();
                self.task(async move { Ok(Msg::LoggedIn(api.login(&user, &buf).await?)) });
            }
            PromptKind::Search => self.do_search(buf),
            PromptKind::Find => {
                self.find = buf;
                self.find_next(1);
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
                } else if let Some((m, s)) = b.split_once(':') {
                    m.parse::<u64>()
                        .ok()
                        .zip(s.parse::<u64>().ok())
                        .map(|(m, s)| (m * 60 + s) * 1000)
                } else {
                    b.parse::<u64>().ok().map(|s| s * 1000)
                };
                match ms {
                    Some(ms) if ms < len => {
                        self.set_pos(ms);
                        self.rpc_fire("core.playback.seek", json!({"time_position": ms}));
                    }
                    _ => self.error("Invalid position"),
                }
            }
            PromptKind::NewPlaylist => {
                if buf.trim().is_empty() {
                    return;
                }
                let api = self.api.clone();
                self.task(async move {
                    api.create_playlist(buf.trim()).await?;
                    Ok(Msg::ReloadPlaylists)
                });
            }
            PromptKind::RenamePlaylist(p) => {
                if buf.trim().is_empty() {
                    return;
                }
                let api = self.api.clone();
                self.task(async move {
                    api.rename_playlist(&p, buf.trim()).await?;
                    Ok(Msg::ReloadPlaylists)
                });
            }
            PromptKind::SaveQueue => {
                if buf.trim().is_empty() || self.queue.is_empty() {
                    return;
                }
                let api = self.api.clone();
                let uris: Vec<String> = self.queue.iter().map(|t| t.track.uri.clone()).collect();
                let n = uris.len();
                self.info(format!("Saving {n} tracks…"));
                self.task(async move {
                    let p = api.create_playlist(buf.trim()).await?;
                    for u in uris {
                        api.add_track(p.id, &u).await?;
                    }
                    Ok(Msg::ReloadPlaylists)
                });
            }
            PromptKind::ConfirmClear | PromptKind::ConfirmDeletePlaylist(_) => {}
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
            K::Enter => {
                let p = self.popup.take().unwrap();
                if let Some((id, _)) = p.items.get(p.cursor) {
                    self.add_to_playlist(*id, p.uris);
                }
            }
            _ => {}
        }
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

/// Recursively browse a directory ref collecting track uris (depth-limited).
async fn browse_tracks(rpc: &Rpc, uri: &str, depth: u8) -> Result<Vec<String>> {
    let v = rpc.call("core.library.browse", json!({"uri": uri})).await?;
    let refs: Vec<Ref> = serde_json::from_value(v)?;
    let mut out = vec![];
    for r in refs {
        if r.is_track() {
            out.push(r.uri);
        } else if depth < 2 {
            out.extend(Box::pin(browse_tracks(rpc, &r.uri, depth + 1)).await?);
        }
    }
    Ok(out)
}

/// User id from the JWT `sub` claim, no verification (display only).
fn jwt_sub(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = b64url_decode(payload)?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    let sub = v.get("sub")?;
    Some(
        sub.as_str()
            .map(String::from)
            .unwrap_or_else(|| sub.to_string()),
    )
}

fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = T.iter().position(|t| *t == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}
