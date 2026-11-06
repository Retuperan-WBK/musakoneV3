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

ncmpcpp-style screens: `1` playlist (columns Artist/Track/Title/Album/Time),
`2` now playing (large cover, album artist and current synced lyric), `3` search
engine (the constraint form), `4` browser, `5` media library (Tidal favourite artists
and the local library, three columns), `6` playlist editor (the room's shared playlists;
you edit your own), `7` stats (room / your listening stats), `8` help / key bindings
(also `F1`), `l` large live lyrics (LRCLIB), `Tab` flips playlist/browser.
Playback `p s > < f b`, volume `Left Right + -`, modes `r z y R`,
`Space` select, `a` add selected to..., `S` save queue as playlist, `/ . ,` find, `g`
jump to position, `o` go to playing song, `c`/`C` clear/crop, `i` song info (with album art), `B` select album, `Ctrl-V` select range, `[`/`]` and
`{`/`}` jump by album/artist, `q` quit. `j`/`k` also move.

In the browser, `Right` opens the selected directory and `Left` goes to its parent;
`Enter`/`Backspace` still work. Use `+`/`-` for volume while browsing. The literal
`>`/`<` keys still skip to the next/previous track.

In Search, Reset clears the form and results and returns the cursor to Any.
Pressing `3` while already in Search performs the same Reset: clears the query and
results and focuses Any. Switching back from another screen preserves the cursor.

Help groups key bindings by screen, like ncmpcpp's F1 page. Scroll with `Up`/`Down`,
`j`/`k` or `Page Up`/`Page Down`; press a screen number to leave help.

Colours follow ncmpcpp's default config (yellow main window, yellow-reverse cursor,
green/white/cyan/magenta columns, green progress bar).

## Build

    cd tui && cargo build --release    # target/release/musakone

For local development with the gitignored `music/` MP3s, run `./dev.sh up` then
`./dev.sh tui` from the repository root. This starts an isolated Docker backend with
silent playback, auto-login as `dev`, and a separate TUI config/cache in `.dev/`.
See the root README for rescan, rebuild and shutdown commands.

## Album art and terminals

UI red uses the standard ANSI red sequence (`31`), avoiding Apple Terminal's
different rendering of indexed red (`38;5;1`). Artwork keeps its indexed/RGB colours.

Now playing (`2`), song info (`i`) and the search preview show the track's album art.
On a truecolor terminal, the current renderer uses coloured half-block characters
(two vertical pixels per character cell). This limits image detail even when the
original cover is high-resolution; it is not a reduced-quality download. Native
inline-image protocols such as iTerm2/Kitty are not implemented yet.

On a Linux text console, writable `/dev/fb0` enables full-pixel framebuffer art;
otherwise the renderer uses dithered 16-colour characters. `MUSAKONE_ASCII=1`
forces plain ASCII for mono terminals, `=0` forces truecolor half-blocks.

The framebuffer also draws lyrics using the bundled Noto Sans Mono TrueType font,
with antialiased pixels rather than block letters. No server font installation is
needed. `musakone --font-license` prints the embedded font's SIL Open Font License.

Local `cover.png`/JPEG artwork is served through the backend's `/local/` proxy, so
the TUI still uses only the backend address (no separate Mopidy URL). After adding
covers, rescan with `./dev.sh scan --force` and restart the TUI to clear cached
missing-art results. The proxy requires the matching backend update.

Songs queued from Files (`file://`) also use the scanned Local media cover for
their equivalent relative path. This requires Mopidy-Local to have indexed that
music root; no extra audio scan or change to the queued song URI is needed.

## Now playing

`2` follows the current song automatically, with a large cover, title, album and
album artist (falling back to the track artist when album-artist tags are missing).
The cover is centered above the title, artist/album details and current lyric,
with the normal terminal header, progress bar, colours and playback keys.

When LRCLIB has timestamped lyrics, the current line follows playback, pause and
seek. On the Linux framebuffer it uses a real font at a moderate size (up to 28px
on an 8x16 console); `l` uses up to 48px for the current line and 28px for upcoming
lines. Tab `2` reserves a fixed five-row lyric
area, so its cover and track details do not move between short, long or missing
lyrics. `l` opens a karaoke-style view with
the current line and two larger, quieter upcoming lines—no previous line or full
song. Each line has a fixed slot, even at the beginning or end of the song. It always
follows playback; scrolling no longer moves away from the current line.

The font wraps and shrinks only when necessary to fit its reserved area. Apple
Terminal and sessions without framebuffer access use normal-sized terminal text,
with the current line highlighted. Unsupported scripts also use terminal text. Finnish/Swedish
characters are supported. Plain untimed lyrics are not displayed
in either live view. Press `l` again to return, or backtick to retry a lyric lookup.
Missing covers or synced lyrics do not prevent the rest of the screen from working.

## Preview Linux rendering on a Mac

From the repository root:

```sh
./dev.sh preview --open
./dev.sh preview --open --size 1280x800 --cell 8x16 --text "Hyvää yötä, Åbo!"
./dev.sh preview --open --cover "music/Nightwish/Century Child/cover.png"
```

This writes Now Playing and Live Lyrics PNGs into a new folder under `.dev/preview/`
and opens them in Preview on macOS. Omit `--open` to only save images. Each run gets
its own folder, so previous previews are preserved. It needs no Docker, login,
network connection or playing music and does not touch normal TUI configuration.

The preview runs the same screens, font rasterizer, geometry and RGB overlay bytes
as the Linux framebuffer, with simulated console cells and clearly labeled demo
lyrics. It tests visual quality and layout, not access to `/dev/fb0` or the real
device's display driver. Set `--size` and `--cell` to approximate that console.

## Stats

`7` shows room or personal listening stats from the backend. `←`/`→` change the window
(7d / 30d / all), `Space` toggles room/you, `u` refreshes, `j`/`k` scroll.

## Performance

Browse loads metadata only for visible tracks and a screen ahead. Cached folders restore
immediately; `u` refreshes them. Artwork and real-font lyrics are cached by size/content.
See [performance notes and benchmarks](../docs/performance.md) for limits and measurements.
