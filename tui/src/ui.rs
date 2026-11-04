//! Rendering, modelled on ncmpcpp's classic design with its default config:
//! header (bold title, " Volume: n%"), state line with [flags], main window in yellow,
//! cursor = yellow reverse (white reverse in inactive columns), progress bar "=>",
//! statusbar "Playing: song" + "[elapsed/total]".
use crate::app::*;
use crate::model::{fmt_long, fmt_ms, Track};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MAIN: Color = Color::Yellow; // main_window_color
const EMPTY_TAG: Color = Color::Cyan; // empty_tag_color
const SELECTED: Color = Color::Magenta; // selected_item_prefix $6
const PROGRESS: Color = Color::DarkGray; // progressbar_color black:b
const PROGRESS_ELAPSED: Color = Color::Green; // progressbar_elapsed_color green:b

struct Row<'a> {
    spans: Vec<Span<'a>>,
    right: Vec<Span<'a>>,
    sep: bool,
    bold: bool,
}

impl<'a> Row<'a> {
    fn text(s: impl Into<String>) -> Row<'a> {
        Row {
            spans: vec![Span::raw(s.into())],
            right: vec![],
            sep: false,
            bold: false,
        }
    }
    fn sep() -> Row<'a> {
        Row {
            spans: vec![],
            right: vec![],
            sep: true,
            bold: false,
        }
    }
    fn song(t: &Track, in_queue: bool) -> Row<'a> {
        // song_list_format = {%a - }{%t}|{$8%f$9}$R{$3%l$9}
        let left = if t.name.is_empty() {
            Span::styled(t.filename(), Style::default().fg(Color::White))
        } else {
            Span::raw(t.classic())
        };
        let right = if t.length.is_some() {
            vec![Span::styled(
                t.duration(),
                Style::default().fg(Color::Green),
            )]
        } else {
            vec![]
        };
        Row {
            spans: vec![left],
            right,
            sep: false,
            bold: in_queue,
        }
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.height < 5 || area.width < 10 {
        return;
    }
    let w = area.width;
    let header = Rect {
        x: 0,
        y: 0,
        width: w,
        height: 1,
    };
    let state_line = Rect {
        x: 0,
        y: 1,
        width: w,
        height: 1,
    };
    let main = Rect {
        x: 0,
        y: 2,
        width: w,
        height: area.height - 4,
    };
    let bar = Rect {
        x: 0,
        y: area.height - 2,
        width: w,
        height: 1,
    };
    let status = Rect {
        x: 0,
        y: area.height - 1,
        width: w,
        height: 1,
    };

    // Menus need their window height before scrolling can be resolved.
    size_menus(app, main);

    draw_header(f, header, state_line, app);
    match app.screen {
        Screen::Help => draw_help(f, main, app),
        Screen::Playlist => draw_playlist(f, main, app),
        Screen::Browser => draw_browser(f, main, app),
        Screen::Search => draw_search(f, main, app),
        Screen::Library => draw_library(f, main, app),
        Screen::Editor => draw_editor(f, main, app),
        Screen::Lyrics => draw_lyrics(f, main, app),
    }
    draw_progress(f, bar, app);
    draw_statusbar(f, status, app);
    if let Some(p) = &app.popup {
        draw_popup(f, area, p);
    }
}

fn size_menus(app: &mut App, main: Rect) {
    let h = main.height as usize;
    app.queue.height = h.saturating_sub(2).max(1); // column titles take two rows
    app.queue.fix_offset();
    app.browser.height = h;
    app.browser.fix_offset();
    app.se.height = h;
    app.se.fix_offset();
    app.lib_artists.height = h.saturating_sub(2).max(1);
    app.lib_artists.fix_offset();
    app.lib_albums.height = h.saturating_sub(2).max(1);
    app.lib_albums.fix_offset();
    app.lib_songs.height = h.saturating_sub(2).max(1);
    app.lib_songs.fix_offset();
    app.playlists.height = h.saturating_sub(2).max(1);
    app.playlists.fix_offset();
    app.content.height = h.saturating_sub(2).max(1);
    app.content.fix_offset();
}

// ---------- header / footer ----------

/// writeCyclicBuffer: text wider than `width` scrolls with " ** " between repeats.
fn scroller(text: &str, width: usize, begin: usize) -> String {
    let tw = text.width();
    if tw <= width {
        return text.to_string();
    }
    let cyc: Vec<char> = format!("{text} ** ").chars().collect();
    let start = begin % cyc.len();
    let mut out = String::new();
    let mut used = 0;
    let mut i = start;
    while used < width {
        let c = cyc[i % cyc.len()];
        let cw = c.width().unwrap_or(0);
        if used + cw > width {
            break;
        }
        out.push(c);
        used += cw;
        i += 1;
    }
    out
}

fn volume_state(app: &App) -> String {
    format!(" Volume: {}%", app.volume)
}

fn draw_header(f: &mut Frame, header: Rect, state_line: Rect, app: &App) {
    let vol = volume_state(app);
    let avail = (header.width as usize).saturating_sub(vol.width());
    let title = match app.screen {
        Screen::Help => "Help".to_string(),
        Screen::Playlist => {
            let n = app.queue.len();
            let mut stats = format!("({n} {}", if n == 1 { "item" } else { "items" });
            if app.queue_len_ms > 0 {
                stats += &format!(", length: {}", fmt_long(app.queue_len_ms));
            }
            stats.push(')');
            format!(
                "Playlist {}",
                scroller(&stats, avail.saturating_sub(9), app.title_scroll)
            )
        }
        Screen::Browser => format!(
            "Browse: {}",
            scroller(
                &app.browse_path(),
                avail.saturating_sub(8),
                app.title_scroll
            )
        ),
        Screen::Search => "Search engine".into(),
        Screen::Library => "Media library".into(),
        Screen::Editor => "Playlist editor".into(),
        Screen::Lyrics => match &app.current {
            Some(c) => format!(
                "Lyrics: {}",
                scroller(
                    &c.track.classic(),
                    avail.saturating_sub(8),
                    app.title_scroll
                )
            ),
            None => "Lyrics".into(),
        },
    };
    let conn = if !app.connected {
        "  [disconnected]"
    } else if !app.mopidy_ok {
        "  [mopidy down]"
    } else {
        ""
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(conn, Style::default().fg(Color::Red)),
        ])),
        Rect {
            width: avail as u16,
            ..header
        },
    );
    f.render_widget(
        Paragraph::new(vol),
        Rect {
            x: header.x + avail as u16,
            width: header.width - avail as u16,
            ..header
        },
    );

    // State line: horizontal line with [flags] near the right edge (flags bold).
    let mut flags = String::new();
    for (on, c) in [
        (app.repeat, 'r'),
        (app.random, 'z'),
        (app.single, 'y'),
        (app.consume, 'R'),
    ] {
        if on {
            flags.push(c);
        }
    }
    let w = state_line.width as usize;
    if flags.is_empty() {
        f.render_widget(Paragraph::new("─".repeat(w)), state_line);
    } else {
        let x = w.saturating_sub(flags.len() + 3);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(format!("{}[", "─".repeat(x))),
                Span::styled(flags.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("]─"),
            ])),
            state_line,
        );
    }
}

fn draw_progress(f: &mut Frame, area: Rect, app: &App) {
    let w = area.width as usize;
    let total = app
        .current
        .as_ref()
        .and_then(|c| c.track.length)
        .filter(|l| *l > 0);
    let (Some(total), true) = (total, app.state != PlayState::Stopped) else {
        return f.render_widget(
            Paragraph::new("─".repeat(w)).style(Style::default().fg(PROGRESS)),
            area,
        );
    };
    let howlong = (w as u64 * app.elapsed_ms().min(total) / total) as usize;
    let mut done = "=".repeat(howlong.min(w));
    if howlong < w {
        done.push('>');
    }
    let rest = w.saturating_sub(done.width());
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                done,
                Style::default()
                    .fg(PROGRESS_ELAPSED)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(rest), Style::default().fg(PROGRESS)),
        ])),
        area,
    );
}

fn draw_statusbar(f: &mut Frame, area: Rect, app: &App) {
    if let Some(p) = &app.prompt {
        let shown = if p.masked {
            "*".repeat(p.buf.chars().count())
        } else {
            p.buf.clone()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(p.label.clone()),
                Span::raw(shown.clone()),
            ])),
            area,
        );
        // Cursor at its byte offset within the (possibly masked) buffer
        let before = if p.masked {
            "*".repeat(p.buf[..p.cursor].chars().count())
        } else {
            p.buf[..p.cursor].to_string()
        };
        let x = area.x + (p.label.width() + before.width()) as u16;
        f.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
        return;
    }
    if let Some((m, _)) = &app.message {
        return f.render_widget(Paragraph::new(m.as_str()), area);
    }
    let Some(cur) = &app.current else { return };
    let ps = match app.state {
        PlayState::Playing => "Playing:",
        PlayState::Paused => "Paused:",
        PlayState::Stopped => return,
    };
    let time = match cur.track.length {
        Some(l) => format!("[{}/{}]", fmt_ms(app.elapsed_ms().min(l)), fmt_ms(l)),
        None => format!("[{}]", fmt_ms(app.elapsed_ms())),
    };
    let avail = (area.width as usize).saturating_sub(ps.len() + time.len() + 2);
    let song = scroller(&cur.track.status(), avail, app.status_scroll);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(ps, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::raw(song),
        ])),
        Rect {
            width: area.width - time.len() as u16,
            ..area
        },
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            time.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Rect {
            x: area.x + area.width - time.len() as u16,
            width: time.len() as u16,
            ..area
        },
    );
}

// ---------- generic menu ----------

/// NC::Menu::refresh — only the visible slice is built.
#[allow(clippy::too_many_arguments)]
fn draw_menu<'a>(
    f: &mut Frame,
    area: Rect,
    len: usize,
    offset: usize,
    cursor: usize,
    highlight: Option<Color>,
    is_sel: impl Fn(usize) -> bool,
    mut row: impl FnMut(usize) -> Row<'a>,
) {
    let w = area.width as usize;
    let mut lines = Vec::with_capacity(area.height as usize);
    for i in offset..(offset + area.height as usize).min(len) {
        let r = row(i);
        if r.sep {
            lines.push(Line::styled("─".repeat(w), Style::default().fg(MAIN)));
            continue;
        }
        let mut base = Style::default().fg(MAIN);
        let hl = highlight.is_some() && i == cursor;
        if hl {
            base = base.fg(highlight.unwrap()).add_modifier(Modifier::REVERSED);
        }
        let selected = is_sel(i);
        if selected {
            base = base.fg(SELECTED);
        }
        if r.bold {
            base = base.add_modifier(Modifier::BOLD);
        }
        let restyle = |s: Span<'a>| -> Span<'a> {
            let mut st = base;
            // discard_colors_if_item_is_selected: selected rows are plain magenta
            if !selected {
                if let Some(fg) = s.style.fg {
                    st = st.fg(fg);
                }
            }
            if s.style.add_modifier.contains(Modifier::BOLD) {
                st = st.add_modifier(Modifier::BOLD);
            }
            Span::styled(s.content, st)
        };
        let right: Vec<Span> = r.right.into_iter().map(restyle).collect();
        let rw: usize = right.iter().map(|s| s.content.width()).sum();
        let lw = w.saturating_sub(rw);
        let mut spans = clip(r.spans.into_iter().map(restyle).collect(), lw);
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        spans.push(Span::styled(" ".repeat(w.saturating_sub(used + rw)), base));
        spans.extend(right);
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// Clip spans to `width` cells (ncurses clears to EOL, no ellipsis).
fn clip<'a>(spans: Vec<Span<'a>>, width: usize) -> Vec<Span<'a>> {
    let mut out = vec![];
    let mut used = 0;
    for span in spans {
        let sw = span.content.width();
        if used + sw <= width {
            used += sw;
            out.push(span);
            continue;
        }
        let mut s = String::new();
        for c in span.content.chars() {
            let cw = c.width().unwrap_or(0);
            if used + cw > width {
                break;
            }
            used += cw;
            s.push(c);
        }
        out.push(Span::styled(s, span.style));
        break;
    }
    out
}

/// Window title (bold, main colour) plus the separator line under it.
fn titled(f: &mut Frame, area: Rect, title: &str) -> Rect {
    f.render_widget(
        Paragraph::new(title.to_string())
            .style(Style::default().fg(MAIN).add_modifier(Modifier::BOLD)),
        Rect { height: 1, ..area },
    );
    f.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(MAIN)),
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        },
    );
    Rect {
        y: area.y + 2,
        height: area.height.saturating_sub(2),
        ..area
    }
}

fn vsep(f: &mut Frame, x: u16, area: Rect) {
    let lines: Vec<Line> = (0..area.height).map(|_| Line::raw("│")).collect();
    f.render_widget(
        Paragraph::new(lines).style(Style::default().fg(MAIN)),
        Rect {
            x,
            width: 1,
            ..area
        },
    );
}

// ---------- playlist (columns mode) ----------

struct Col {
    width: usize,
    fixed: bool,
    name: &'static str,
    color: Option<Color>,
    stretch: Option<usize>,
}

// song_columns_list_format = (20)[]{a} (6f)[green]{NE} (50)[white]{t|f:Title} (20)[cyan]{b} (7f)[magenta]{l}
const COLUMNS: [Col; 5] = [
    Col {
        width: 20,
        fixed: false,
        name: "Artist",
        color: None,
        stretch: None,
    },
    Col {
        width: 6,
        fixed: true,
        name: "Track",
        color: Some(Color::Green),
        stretch: None,
    },
    Col {
        width: 50,
        fixed: false,
        name: "Title",
        color: Some(Color::White),
        stretch: None,
    },
    Col {
        width: 20,
        fixed: false,
        name: "Album",
        color: Some(Color::Cyan),
        stretch: Some(7),
    },
    Col {
        width: 7,
        fixed: true,
        name: "Time",
        color: Some(Color::Magenta),
        stretch: None,
    },
];

/// Display::Columns width rule: percent of the window, fixed cells, one space between,
/// the last relative column stretches to fill what the fixed ones after it leave.
fn column_widths(total: usize) -> Vec<usize> {
    let mut out = vec![];
    let mut remained = total as isize;
    let last = COLUMNS.len() - 1;
    for (i, c) in COLUMNS.iter().enumerate() {
        let mut width: isize = match c.stretch {
            Some(s) => remained - s as isize,
            None => {
                if c.fixed {
                    c.width as isize
                } else {
                    (c.width * total / 100) as isize
                }
            }
        };
        if width == 0 {
            out.push(0);
            continue;
        }
        if i != last {
            width -= 1;
        }
        if remained - width < 0 || width < 0 {
            break;
        }
        out.push(width as usize);
        if i != last {
            remained -= width + 1;
        }
    }
    out
}

fn pad(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw > width {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push_str(&" ".repeat(width - used));
    out
}

fn columns_header(width: usize) -> String {
    let ws = column_widths(width);
    let mut s = String::new();
    for (i, w) in ws.iter().enumerate() {
        if *w == 0 {
            continue;
        }
        s.push_str(&pad(COLUMNS[i].name, *w));
        if i + 1 < ws.len() {
            s.push(' ');
        }
    }
    s
}

fn song_columns<'a>(t: &Track, width: usize) -> Vec<Span<'a>> {
    let ws = column_widths(width);
    let empty = || Span::styled("<empty>".to_string(), Style::default().fg(EMPTY_TAG));
    let mut spans = vec![];
    for (i, w) in ws.iter().enumerate() {
        if *w == 0 {
            continue;
        }
        let tag: Option<String> = match i {
            0 => (!t.artists.is_empty()).then(|| t.artists.clone()),
            1 => Some(t.track_no.map(|n| n.to_string()).unwrap_or_default()), // E: no empty marker
            2 => Some(if t.name.is_empty() {
                t.filename()
            } else {
                t.name.clone()
            }),
            3 => (!t.album.is_empty()).then(|| t.album.clone()),
            _ => Some(t.duration()),
        };
        let style = COLUMNS[i]
            .color
            .map(|c| Style::default().fg(c))
            .unwrap_or_default();
        match tag {
            Some(v) => spans.push(Span::styled(pad(&v, *w), style)),
            None => {
                let e = empty();
                let ew = e.content.width();
                if ew <= *w {
                    spans.push(e);
                    spans.push(Span::raw(" ".repeat(w - ew)));
                } else {
                    spans.push(Span::styled(
                        pad("<empty>", *w),
                        Style::default().fg(EMPTY_TAG),
                    ));
                }
            }
        }
        if i + 1 < ws.len() {
            spans.push(Span::raw(" "));
        }
    }
    spans
}

fn draw_playlist(f: &mut Frame, area: Rect, app: &App) {
    let body = titled(f, area, &columns_header(area.width as usize));
    let cur = app.current_tlid();
    let hl = app.playlist_highlight().then_some(MAIN);
    let q = &app.queue;
    draw_menu(
        f,
        body,
        q.len(),
        q.offset,
        q.cursor,
        hl,
        |i| q.is_sel(i),
        |i| {
            let t = &q.items[i];
            Row {
                spans: song_columns(&t.track, body.width as usize),
                right: vec![],
                sep: false,
                bold: Some(t.tlid) == cur,
            }
        },
    );
}

// ---------- browser ----------

fn draw_browser(f: &mut Frame, area: Rect, app: &App) {
    let b = &app.browser;
    if b.is_empty() {
        if app.browse_loading {
            f.render_widget(
                Paragraph::new("Loading...").style(Style::default().fg(MAIN)),
                area,
            );
        }
        return;
    }
    draw_menu(
        f,
        area,
        b.len(),
        b.offset,
        b.cursor,
        Some(MAIN),
        |i| b.is_sel(i),
        |i| {
            let r = &b.items[i];
            if r.is_track() {
                match app.track_of(&r.uri) {
                    Some(t) => Row::song(t, app.queue_uris.contains(&r.uri)),
                    None => Row {
                        spans: vec![Span::raw(r.name.clone())],
                        right: vec![],
                        sep: false,
                        bold: app.queue_uris.contains(&r.uri),
                    },
                }
            } else if r.is_playlist() {
                // browser_playlist_prefix = "$2playlist$9 "
                Row {
                    spans: vec![
                        Span::styled("playlist", Style::default().fg(Color::Red)),
                        Span::raw(format!(" {}", r.name)),
                    ],
                    right: vec![],
                    sep: false,
                    bold: false,
                }
            } else {
                Row::text(format!("[{}]", r.name))
            }
        },
    );
}

// ---------- search engine ----------

fn draw_search(f: &mut Frame, area: Rect, app: &App) {
    let se = &app.se;
    let bold = Style::default().add_modifier(Modifier::BOLD);
    draw_menu(
        f,
        area,
        se.len(),
        se.offset,
        se.cursor,
        Some(MAIN),
        |i| se.is_sel(i),
        |i| match &se.items[i] {
            SeItem::Field(k) => Row {
                spans: vec![
                    Span::styled(SE_FIELDS[*k], bold),
                    Span::raw(format!(": {}", app.se_fields[*k])),
                ],
                right: vec![],
                sep: false,
                bold: false,
            },
            SeItem::Sep => Row::sep(),
            SeItem::SearchIn => Row {
                spans: vec![
                    Span::styled("Search in:", bold),
                    Span::raw(format!(
                        " {}",
                        if app.se_in_db {
                            "Database"
                        } else {
                            "Current playlist"
                        }
                    )),
                ],
                right: vec![],
                sep: false,
                bold: false,
            },
            SeItem::Mode => Row {
                spans: vec![
                    Span::styled("Search mode:", bold),
                    Span::raw(format!(" {}", SE_MODES[app.se_mode])),
                ],
                right: vec![],
                sep: false,
                bold: false,
            },
            SeItem::Search => Row::text(if app.se_searching {
                "Search (searching...)"
            } else {
                "Search"
            }),
            SeItem::Reset => Row::text("Reset"),
            SeItem::Results => {
                // Tab bar like the web UI: songs | artists | albums, active one reversed
                let mut spans = vec![Span::styled("Search results: ", bold)];
                let tabs = [
                    format!(" Songs {} ", app.se_tracks.len()),
                    format!(" Artists {} ", app.se_artists.len()),
                    format!(" Albums {} ", app.se_albums.len()),
                ];
                for (i, t) in tabs.iter().enumerate() {
                    let st = if i == app.se_tab {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    spans.push(Span::styled(t.clone(), st));
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(
                    "(Left/Right to switch)",
                    Style::default().fg(Color::DarkGray),
                ));
                Row {
                    spans,
                    right: vec![],
                    sep: false,
                    bold: false,
                }
            }
            SeItem::Song(t) => Row::song(t, app.queue_uris.contains(&t.uri)),
            SeItem::Artist(r) => Row::text(r.name.clone()),
            SeItem::Album(r) => Row::text(format!("[{}]", r.name)),
        },
    );
}

// ---------- media library ----------

fn draw_library(f: &mut Frame, area: Rect, app: &App) {
    let w = area.width;
    let left_w = w / 3 - 1;
    let mid_x = left_w + 1;
    let mid_w = w / 3 - 1;
    let right_x = mid_x + mid_w + 1;
    let right_w = w - right_x;
    let cols = [
        Rect {
            x: area.x,
            width: left_w,
            ..area
        },
        Rect {
            x: area.x + mid_x,
            width: mid_w,
            ..area
        },
        Rect {
            x: area.x + right_x,
            width: right_w,
            ..area
        },
    ];
    vsep(f, area.x + left_w, area);
    vsep(f, area.x + right_x - 1, area);
    let hl = |col: usize| {
        Some(if app.lib_col == col {
            MAIN
        } else {
            Color::White
        })
    };

    let body = titled(f, cols[0], "Artists");
    let m = &app.lib_artists;
    if m.is_empty() && app.lib_loading {
        f.render_widget(
            Paragraph::new("Loading...").style(Style::default().fg(MAIN)),
            body,
        );
    }
    draw_menu(
        f,
        body,
        m.len(),
        m.offset,
        m.cursor,
        hl(0),
        |i| m.is_sel(i),
        |i| Row::text(m.items[i].name.clone()),
    );

    let body = titled(f, cols[1], "Albums");
    let m = &app.lib_albums;
    draw_menu(
        f,
        body,
        m.len(),
        m.offset,
        m.cursor,
        hl(1),
        |i| m.is_sel(i),
        |i| {
            let e = &m.items[i];
            Row::text(if e.all {
                "All tracks".to_string()
            } else {
                e.r.as_ref().map(|r| r.name.clone()).unwrap_or_default()
            })
        },
    );

    let body = titled(f, cols[2], "Songs");
    let m = &app.lib_songs;
    draw_menu(
        f,
        body,
        m.len(),
        m.offset,
        m.cursor,
        hl(2),
        |i| m.is_sel(i),
        |i| {
            let t = &m.items[i];
            Row {
                spans: vec![Span::raw(t.library())],
                right: vec![],
                sep: false,
                bold: app.queue_uris.contains(&t.uri),
            }
        },
    );
}

// ---------- playlist editor ----------

fn draw_editor(f: &mut Frame, area: Rect, app: &App) {
    let w = area.width;
    let left_w = w / 3 - 1; // playlist_editor_column_width_ratio = 1:2
    let right_x = left_w + 1;
    let left = Rect {
        width: left_w,
        ..area
    };
    let right = Rect {
        x: area.x + right_x,
        width: w - right_x,
        ..area
    };
    vsep(f, area.x + left_w, area);

    let body = titled(f, left, "Playlists");
    let m = &app.playlists;
    if m.is_empty() && app.user_id.is_none() {
        f.render_widget(
            Paragraph::new("Not logged in (L)").style(Style::default().fg(MAIN)),
            body,
        );
    }
    draw_menu(
        f,
        body,
        m.len(),
        m.offset,
        m.cursor,
        Some(if app.ed_col == 0 { MAIN } else { Color::White }),
        |i| m.is_sel(i),
        |i| {
            let p = &m.items[i];
            let own = app.user_id == Some(p.user_id);
            Row {
                spans: vec![Span::raw(p.name.clone())],
                right: if own {
                    vec![]
                } else {
                    vec![Span::styled(
                        p.username.clone(),
                        Style::default().fg(Color::DarkGray),
                    )]
                },
                sep: false,
                bold: false,
            }
        },
    );

    let body = titled(f, right, "Content");
    let m = &app.content;
    draw_menu(
        f,
        body,
        m.len(),
        m.offset,
        m.cursor,
        Some(if app.ed_col == 1 { MAIN } else { Color::White }),
        |i| m.is_sel(i),
        |i| {
            let u = &m.items[i];
            match app.track_of(u) {
                Some(t) => Row::song(t, app.queue_uris.contains(u)),
                None => Row::text(u.clone()),
            }
        },
    );
}

// ---------- lyrics ----------

fn draw_lyrics(f: &mut Frame, area: Rect, app: &mut App) {
    let style = Style::default().fg(MAIN);
    let lines: Vec<Line> = match &app.lyrics {
        LyricsState::Idle => vec![],
        LyricsState::Fetching(_) => vec![Line::from(vec![
            Span::raw("Fetching lyrics from "),
            Span::styled("lrclib", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("... "),
        ])],
        LyricsState::Loaded(_, l) if l.is_empty() => {
            vec![Line::raw(""), Line::raw("Lyrics were not found.")]
        }
        LyricsState::Loaded(_, l) => {
            let active = app.lyrics_active();
            if app.lyrics_follow {
                if let Some(a) = active {
                    app.lyrics_offset = a.saturating_sub(area.height as usize / 2);
                }
            }
            let max_off = l.len().saturating_sub(area.height as usize);
            app.lyrics_offset = app.lyrics_offset.min(max_off);
            l.iter()
                .enumerate()
                .skip(app.lyrics_offset)
                .take(area.height as usize)
                .map(|(i, (_, text))| {
                    if Some(i) == active {
                        Line::styled(
                            text.clone(),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Line::raw(text.clone())
                    }
                })
                .collect()
        }
    };
    if app.current.is_none() {
        return f.render_widget(Paragraph::new("").style(style), area);
    }
    f.render_widget(Paragraph::new(lines).style(style), area);
}

// ---------- help ----------

const HELP_SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Keys - Movement",
        &[
            ("Up k", "Move cursor up"),
            ("Down j", "Move cursor down"),
            ("Page Up", "Page up"),
            ("Page Down", "Page down"),
            ("Home", "Home"),
            ("End", "End"),
            ("", ""),
            ("Tab", "Switch to next screen in sequence"),
            ("Shift-Tab", "Switch to previous screen in sequence"),
            ("F1", "Show help"),
            ("1", "Show playlist"),
            ("2", "Show browser"),
            ("3", "Show search engine"),
            ("4", "Show media library"),
            ("5", "Show playlist editor"),
        ],
    ),
    (
        "Keys - Global",
        &[
            ("s", "Stop"),
            ("p", "Pause"),
            (">", "Next track"),
            ("<", "Previous track"),
            ("Backspace", "Replay current song"),
            ("f", "Seek forward in playing song"),
            ("b", "Seek backward in playing song"),
            ("Left -", "Decrease volume by 2%"),
            ("Right +", "Increase volume by 2%"),
            ("", ""),
            ("v", "Reverse selection"),
            ("V", "Remove selection"),
            ("Space", "Select current item"),
            ("a", "Add selected items to playlist"),
            ("", ""),
            ("r", "Toggle repeat mode"),
            ("z", "Toggle random mode"),
            ("y", "Toggle single mode"),
            ("R", "Toggle consume mode"),
            ("u", "Start music database update"),
            ("", ""),
            ("/", "Find item forward"),
            ("?", "Find item backward"),
            (",", "Jump to previous found item"),
            (".", "Jump to next found item"),
            (
                "g",
                "Jump to given position in playing song (formats: mm:ss, x%)",
            ),
            ("l", "Show/hide song lyrics"),
            ("L", "Log in as another user"),
            ("", ""),
            ("q", "Quit"),
        ],
    ),
    (
        "Keys - Playlist",
        &[
            ("Enter", "Play selected item"),
            ("Delete d", "Delete selected item(s) from playlist"),
            ("c", "Clear playlist"),
            ("C", "Clear playlist except selected item(s)"),
            ("m", "Move selected item(s) up"),
            ("n", "Move selected item(s) down"),
            ("A", "Add item to playlist"),
            ("S", "Save playlist"),
            ("Z", "Shuffle range"),
            ("o", "Jump to current song"),
        ],
    ),
    (
        "Keys - Browser",
        &[
            ("Enter", "Enter directory"),
            ("Enter", "Add item to playlist and play it"),
            ("Space", "Add item to playlist"),
            ("Backspace", "Jump to parent directory"),
        ],
    ),
    (
        "Keys - Search engine",
        &[
            ("Enter", "Modify option / Run action"),
            ("Space", "Add item to playlist"),
            ("Enter", "Add item to playlist and play it"),
        ],
    ),
    (
        "Keys - Media library",
        &[
            ("Left", "Previous column"),
            ("Right", "Next column"),
            ("Enter", "Add item to playlist and play it"),
            ("Space", "Add item to playlist"),
        ],
    ),
    (
        "Keys - Playlist editor",
        &[
            ("Left", "Previous column"),
            ("Right", "Next column"),
            ("Enter", "Add item to playlist and play it"),
            ("Space", "Add item to playlist"),
            ("e", "Edit playlist name"),
            ("m", "Move selected item(s) up"),
            ("n", "Move selected item(s) down"),
            ("Delete d", "Delete selected playlists (left column)"),
            (
                "Delete d",
                "Delete selected item(s) from playlist (right column)",
            ),
        ],
    ),
    (
        "Keys - Lyrics",
        &[
            ("o", "Follow the playing song again"),
            ("`", "Refetch lyrics"),
        ],
    ),
];

fn help_lines() -> Vec<Line<'static>> {
    let mut out = vec![];
    for (title, keys) in HELP_SECTIONS {
        out.push(Line::raw(""));
        out.push(Line::styled(
            format!("  {title}"),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        out.push(Line::raw(""));
        for (k, d) in keys.iter() {
            if k.is_empty() {
                out.push(Line::raw(""));
            } else {
                out.push(Line::raw(format!("    {:<20} : {d}", k)));
            }
        }
    }
    out
}

pub fn help_len() -> usize {
    help_lines().len()
}

fn draw_help(f: &mut Frame, area: Rect, app: &App) {
    let lines: Vec<Line> = help_lines()
        .into_iter()
        .skip(app.help_offset)
        .take(area.height as usize)
        .collect();
    f.render_widget(Paragraph::new(lines).style(Style::default().fg(MAIN)), area);
}

// ---------- popup (sel_items_adder) ----------

fn draw_popup(f: &mut Frame, area: Rect, p: &Popup) {
    let h = (p.items.len() as u16 + 2)
        .min(area.height.saturating_sub(4))
        .max(3);
    let w = (p
        .items
        .iter()
        .map(|n| n.width())
        .max()
        .unwrap_or(10)
        .max(p.title.width()) as u16
        + 4)
    .clamp(24, area.width.saturating_sub(4));
    let r = Rect {
        x: (area.width - w) / 2,
        y: (area.height - h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(p.title.as_str())
        .border_style(Style::default().fg(Color::Red)) // active_window_border = red
        .style(Style::default().fg(MAIN));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let vis = inner.height as usize;
    let offset = p.cursor.saturating_sub(vis.saturating_sub(1));
    let lines: Vec<Line> = p
        .items
        .iter()
        .enumerate()
        .skip(offset)
        .take(vis)
        .map(|(i, n)| {
            let l = Line::raw(pad(n, inner.width as usize));
            if i == p.cursor {
                l.style(Style::default().fg(MAIN).add_modifier(Modifier::REVERSED))
            } else {
                l
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fill_the_width_like_ncmpcpp() {
        // 100 cells: 20%->19(+1 space), 6f->5(+1), 50%->49(+1), album stretches, 7f last.
        let ws = column_widths(100);
        assert_eq!(ws, vec![19, 5, 49, 100 - (20 + 6 + 50) - 7 - 1, 7]);
        let total: usize = ws.iter().sum::<usize>() + (ws.len() - 1);
        assert_eq!(total, 100);
        assert_eq!(columns_header(100).width(), 100);
        assert_eq!(scroller("abc", 5, 3), "abc");
        assert_eq!(scroller("abcdef", 4, 2), "cdef");
    }
}
