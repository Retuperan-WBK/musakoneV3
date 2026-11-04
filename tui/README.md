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

First start asks for your musakone username and password; the token is kept in
`~/.config/musakone/token`. Esc skips login (anonymous plays, no playlists).

## Keys

Press `1` or `F1` inside the app. Same as ncmpcpp where there is an equivalent:
`2` queue, `3` browse, `4` search, `6` playlist editor, `l` lyrics, `5` Mix
(recommendations and autoplay, replaces ncmpcpp's media library).

## Build

    cd tui && cargo build --release    # target/release/musakone
