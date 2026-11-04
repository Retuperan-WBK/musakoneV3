# musakone (TUI)

ncmpcpp-style terminal client for the MusakoneV3 backend. Talks only to the Gleam
backend: Mopidy JSON-RPC over its `/ws` proxy (so plays are attributed to you),
REST for login, playlists and Mix/autoplay. Lyrics come from lrclib.net.

## Install / update

    curl -fsSL https://github.com/Retuperan-WBK/musakoneV3/releases/download/tui-latest/install.sh | sh

Every push to `master` that touches `tui/` rebuilds a static Linux x86_64 binary and
replaces the `tui-latest` release. An installed client downloads the new build in the
background on start and tells you to restart; `musakone update` does it on demand.

## Run

    musakone                       # backend from ~/.config/musakone/url, default http://10.0.1.2:3001
    musakone https://mb.rwbk.fi    # use and remember another backend

Login: put `MUSAKONE_USER` and `MUSAKONE_PASSWORD` in the environment or in
`~/.config/musakone/env` (KEY=VALUE lines) and the client logs in by itself; otherwise it
asks once and keeps the token in `~/.config/musakone/token`. `L` switches user.

## Screens and keys

ncmpcpp's defaults: `F1` help, `1` playlist (columns Artist/Track/Title/Album/Time),
`2` browser, `3` search engine (the constraint form), `4` media library (Tidal favourite
artists + local library, three columns), `5` playlist editor (the room's shared playlists;
you edit your own), `6` album art (cover rendered in the terminal), `l` lyrics (lrclib.net,
synced lines follow the song), `Tab` flips playlist/browser. Playback `p s > < f b`, volume `Left Right + -`, modes `r z y R`,
`Space` select, `a` add selected to..., `S` save queue as playlist, `/ . ,` find, `g`
jump to position, `o` go to playing song, `c`/`C` clear/crop, `i` song info, `B` select album, `Ctrl-V` select range, `[`/`]` and
`{`/`}` jump by album/artist, `q` quit. `j`/`k` also move.

Colours follow ncmpcpp's default config (yellow main window, yellow-reverse cursor,
green/white/cyan/magenta columns, green progress bar).

## Build

    cd tui && cargo build --release    # target/release/musakone
