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

const MAIN: Color = Color::Red; // main_window_color = red
const EMPTY_TAG: Color = Color::Red; // empty_tag_color = red
const SELECTED: Color = Color::Magenta; // selected_item_prefix $6
const NOW: Color = Color::LightYellow; // now-playing row: bright accent, bold on top
const PROGRESS: Color = Color::DarkGray; // progressbar_color black:b
const PROGRESS_ELAPSED: Color = Color::Green; // progressbar_elapsed_color
const STATUSBAR: Color = Color::Yellow; // statusbar_color = yellow
const STATE_LINE: Color = Color::Red; // state_line_color = red

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
    fn song_cols(t: &Track, width: usize, in_queue: bool) -> Row<'a> {
        Row {
            spans: song_columns(t, width),
            right: vec![],
            sep: false,
            bold: in_queue,
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
    // ncmpcpp "alternative" user interface: a two-line now-playing header on top,
    // a divider carrying the screen name, the body, then progress bar + status.
    let row = |y: u16| Rect {
        x: 0,
        y,
        width: w,
        height: 1,
    };
    let np0 = row(0);
    let np1 = row(1);
    let divider = row(2);
    let main = Rect {
        x: 0,
        y: 3,
        width: w,
        height: area.height - 5,
    };
    let bar = row(area.height - 2);
    let status = row(area.height - 1);

    // Menus need their window height before scrolling can be resolved.
    size_menus(app, main);

    draw_alt_header(f, np0, np1, divider, app);
    match app.screen {
        Screen::Help => draw_help(f, main, app),
        Screen::Playlist => draw_playlist(f, main, app),
        Screen::Browser => draw_browser(f, main, app),
        Screen::Search => draw_search(f, main, app),
        Screen::Library => draw_library(f, main, app),
        Screen::Editor => draw_editor(f, main, app),
        Screen::Lyrics => draw_lyrics(f, main, app),
        Screen::Stats => draw_stats(f, main, app),
    }
    draw_progress(f, bar, app);
    draw_statusbar(f, status, app);
    if let Some(p) = &app.popup {
        draw_popup(f, area, p);
    }
    if let Some((uri, rows)) = &app.info {
        draw_info(f, area, app, uri, rows);
    }
}

/// Song-info overlay (ncmpcpp `i`): album art on the left, label/value tags on the right.
fn draw_info(f: &mut Frame, area: Rect, app: &App, uri: &str, rows: &[(String, String)]) {
    let label_w = rows.iter().map(|(k, _)| k.width()).max().unwrap_or(6);
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(
                    format!("{k:>label_w$}: "),
                    Style::default().fg(MAIN).add_modifier(Modifier::BOLD),
                ),
                Span::raw(v.clone()),
            ])
        })
        .collect();
    let text_w = rows
        .iter()
        .map(|(k, v)| k.width() + 2 + v.width())
        .max()
        .unwrap_or(20) as u16;
    // The art is a square sized to the row count; width in cells ~= 2x the rows.
    let art_rows = rows.len() as u16;
    let art_w = (art_rows * 2).min(area.width / 3);
    let has_art = art_w >= 6 && area.width > text_w + art_w + 8;
    let inner_w = if has_art { art_w + 2 + text_w } else { text_w };
    let w = (inner_w + 4).clamp(24, area.width.saturating_sub(2));
    let h = (rows.len() as u16 + 2).min(area.height.saturating_sub(2));
    let r = Rect {
        x: (area.width - w) / 2,
        y: (area.height - h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Song info")
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().fg(MAIN));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let text_area = if has_art {
        if let Some(Some(img)) = app.art_cache.get(uri) {
            render_cover(
                f,
                Rect {
                    width: art_w,
                    ..inner
                },
                img,
            );
        }
        Rect {
            x: inner.x + art_w + 2,
            width: inner.width.saturating_sub(art_w + 2),
            ..inner
        }
    } else {
        inner
    };
    f.render_widget(Paragraph::new(lines), text_area);
}

fn size_menus(app: &mut App, main: Rect) {
    let h = main.height as usize;
    app.queue.height = h.saturating_sub(2).max(1); // column titles take two rows
    app.queue.fix_offset();
    app.browser.height = h.saturating_sub(2).max(1); // column header + separator
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

fn volume_state(_app: &App) -> String {
    format!(" Vol: {}%", _app.volume)
}

fn screen_title(app: &App, avail: usize) -> String {
    match app.screen {
        Screen::Stats => "Stats".to_string(),
        Screen::Help => "Help".to_string(),
        Screen::Playlist => {
            let n = app.queue.len();
            let mut stats = format!("({n} {}", if n == 1 { "item" } else { "items" });
            if app.queue_len_ms > 0 {
                stats += &format!(", length: {}", fmt_long(app.queue_len_ms));
            }
            stats.push(')');
            format!(
                "Playlist  {}",
                scroller(&stats, avail.saturating_sub(10), app.title_scroll)
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
        Screen::Lyrics => "Lyrics".into(),
    }
}

/// Place left / centered / right segments on a single row.
fn place(f: &mut Frame, r: Rect, left: Line, center: Line, right: Line) {
    let w = r.width as usize;
    let lw = left.width();
    let rw = right.width();
    let cw = center.width();
    if rw > 0 {
        f.render_widget(
            Paragraph::new(right).right_aligned(),
            Rect {
                x: r.x + r.width - rw as u16,
                width: rw as u16,
                ..r
            },
        );
    }
    if lw > 0 {
        f.render_widget(
            Paragraph::new(left),
            Rect {
                width: lw as u16,
                ..r
            },
        );
    }
    if cw > 0 && cw <= w {
        let start = ((w - cw) / 2)
            .max(lw + 1)
            .min(w.saturating_sub(rw + 1 + cw));
        f.render_widget(
            Paragraph::new(center),
            Rect {
                x: r.x + start as u16,
                width: cw as u16,
                ..r
            },
        );
    }
}

/// Alternative-UI header: two now-playing lines plus a divider carrying the screen name.
fn draw_alt_header(f: &mut Frame, np0: Rect, np1: Rect, divider: Rect, app: &App) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let w = divider.width as usize;

    // Row 0: [elapsed/total]  <title>  Vol: X%
    let (time, title): (Line, Line) = match &app.current {
        Some(c) => {
            let t = match c.track.length {
                Some(l) => format!("{}/{}", fmt_ms(app.elapsed_ms().min(l)), fmt_ms(l)),
                None => fmt_ms(app.elapsed_ms()),
            };
            let name = if c.track.name.is_empty() {
                c.track.filename()
            } else {
                c.track.name.clone()
            };
            (
                Line::styled(t, bold),
                Line::styled(scroller(&name, w / 2, app.title_scroll), bold),
            )
        }
        None => (Line::raw(""), Line::raw("")),
    };
    let vol = volume_state(app);
    place(f, np0, time, title, Line::raw(vol));

    // Row 1: [state]  <artist - album (year)>  [flags]
    let state = match app.state {
        PlayState::Playing => "[playing]",
        PlayState::Paused => "[paused]",
        PlayState::Stopped => "[stopped]",
    };
    let mid: Line = match &app.current {
        Some(c) if !c.track.artists.is_empty() => {
            let mut spans = vec![Span::styled(
                c.track.artists.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )];
            if !c.track.album.is_empty() {
                spans.push(Span::raw(" - "));
                spans.push(Span::styled(
                    c.track.album.clone(),
                    Style::default().fg(Color::Cyan),
                ));
            }
            if !c.track.date.is_empty() {
                spans.push(Span::styled(
                    format!(" ({})", c.track.date),
                    Style::default().fg(Color::Yellow),
                ));
            }
            Line::from(spans)
        }
        _ => Line::raw(""),
    };
    // flags: all six ncmpcpp slots, '-' when off. Mopidy has no crossfade/db-update,
    // so those two stay '-' (matches ncmpcpp's [r-----] width against this server).
    let mut fl = String::from("[");
    for (on, c) in [
        (app.repeat, 'r'),
        (app.random, 'z'),
        (app.single, 'y'),
        (app.consume, 'R'),
        (false, 'x'),
        (false, 'U'),
    ] {
        fl.push(if on { c } else { '-' });
    }
    fl.push(']');
    place(
        f,
        np1,
        Line::styled(state.to_string(), bold),
        mid,
        Line::raw(fl),
    );

    // Divider: screen name (bold) then a red rule; append connection trouble in red.
    let conn = if !app.connected {
        "  [disconnected]"
    } else if !app.mopidy_ok {
        "  [mopidy down]"
    } else {
        ""
    };
    let name = screen_title(app, w.saturating_sub(4));
    let used = name.width() + conn.width();
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(name, bold.fg(MAIN)),
            Span::styled(conn, Style::default().fg(Color::Red)),
            Span::styled(
                format!(" {}", "─".repeat(w.saturating_sub(used + 1))),
                Style::default().fg(STATE_LINE),
            ),
        ])),
        divider,
    );
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
    // Alternative UI keeps the now-playing line in the header; the statusbar is
    // only for messages (statusbar_color = yellow).
    if let Some((m, _)) = &app.message {
        f.render_widget(
            Paragraph::new(m.as_str()).style(Style::default().fg(STATUSBAR)),
            area,
        );
    }
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
    let cur_uri = app.current.as_ref().map(|c| c.track.uri.clone());
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
            // Match the playing track by tlid, or uri as a fallback.
            let now = Some(t.tlid) == cur
                || (cur.is_none() && cur_uri.as_deref() == Some(t.track.uri.as_str()));
            let mut spans = song_columns(&t.track, body.width as usize);
            if now {
                // ncmpcpp marks it bold; plain bold is too subtle, so the whole row also
                // takes the bright accent colour so the playing track is obvious.
                for s in &mut spans {
                    s.style = s.style.fg(NOW);
                }
            }
            Row {
                spans,
                right: vec![],
                sep: false,
                bold: now,
            }
        },
    );
}

// ---------- browser ----------

fn draw_browser(f: &mut Frame, area: Rect, app: &App) {
    // browser_display_mode = columns
    let body = titled(f, area, &columns_header(area.width as usize));
    let b = &app.browser;
    if b.is_empty() {
        if app.browse_loading {
            f.render_widget(
                Paragraph::new("Loading...").style(Style::default().fg(MAIN)),
                body,
            );
        }
        return;
    }
    let cw = body.width as usize;
    draw_menu(
        f,
        body,
        b.len(),
        b.offset,
        b.cursor,
        Some(MAIN),
        |i| b.is_sel(i),
        |i| {
            let r = &b.items[i];
            if r.is_track() {
                match app.track_of(&r.uri) {
                    Some(t) => Row::song_cols(t, cw, app.queue_uris.contains(&r.uri)),
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
    // When the cursor is on a song and there's room, show a preview panel on the right
    // with that song's album art and full tags.
    let hovered = match se.cur() {
        Some(SeItem::Song(t)) => Some(t.clone()),
        _ => None,
    };
    let (list_area, panel) = match &hovered {
        Some(_) if area.width >= 72 => {
            let pw = (area.width / 3).clamp(30, 50);
            let list = Rect {
                width: area.width - pw - 1,
                ..area
            };
            let panel = Rect {
                x: area.x + area.width - pw,
                width: pw,
                ..area
            };
            vsep(f, area.x + area.width - pw - 1, area);
            (list, Some(panel))
        }
        _ => (area, None),
    };
    if let (Some(p), Some(t)) = (panel, &hovered) {
        draw_search_preview(f, p, app, t);
    }
    // search_engine_display_mode = columns (song results align like the playlist)
    let cw = list_area.width as usize;
    let bold = Style::default().add_modifier(Modifier::BOLD);
    draw_menu(
        f,
        list_area,
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
            SeItem::Song(t) => Row::song_cols(t, cw, app.queue_uris.contains(&t.uri)),
            SeItem::Artist(r) => Row::text(r.name.clone()),
            SeItem::Album(r) => Row::text(format!("[{}]", r.name)),
        },
    );
}

/// Right-hand preview for the hovered search song: album art on top, tags below.
fn draw_search_preview(f: &mut Frame, area: Rect, app: &App, t: &Track) {
    // Album art fills a square at the top (its height ~ panel width / 2 in cells).
    let art_h = (area.width / 2).min(area.height.saturating_sub(8));
    if art_h > 0 {
        let art_area = Rect {
            height: art_h,
            ..area
        };
        match art_for(app, &t.uri) {
            Some(Some(img)) => render_cover(f, art_area, img),
            Some(None) => f.render_widget(
                Paragraph::new("\n(no album art)")
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                art_area,
            ),
            None => f.render_widget(
                Paragraph::new("\nloading art...")
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                art_area,
            ),
        }
    }
    // Tags below the art.
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let field = |k: &str, v: &str, lines: &mut Vec<Line>| {
        if !v.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("{k}: "), bold.fg(MAIN)),
                Span::raw(v.to_string()),
            ]));
        }
    };
    let mut lines = vec![Line::raw("")];
    field("Title", &t.name, &mut lines);
    field("Artist", &t.artists, &mut lines);
    field("Album", &t.album, &mut lines);
    field("Date", &t.date, &mut lines);
    field(
        "Track",
        &t.track_no.map(|n| n.to_string()).unwrap_or_default(),
        &mut lines,
    );
    field("Length", &t.duration(), &mut lines);
    let src = if t.uri.starts_with("tidal:") {
        "Tidal"
    } else if t.uri.starts_with("local:") || t.uri.starts_with("file:") {
        "Local"
    } else {
        ""
    };
    field("Source", src, &mut lines);
    if app.queue_uris.contains(&t.uri) {
        lines.push(Line::styled("in queue", Style::default().fg(Color::Green)));
    }
    let info_area = Rect {
        y: area.y + art_h,
        height: area.height.saturating_sub(art_h),
        ..area
    };
    f.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }),
        info_area,
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

/// Album art rendered with Unicode upper-half-blocks: two vertical pixels per cell,
/// foreground = top pixel, background = bottom pixel, so each cell shows two colours.
/// Art state for a uri: Some(Some(img)) ready, Some(None) no art, None still loading.
fn art_for<'a>(app: &'a App, uri: &str) -> Option<&'a Option<image::DynamicImage>> {
    app.art_cache.get(uri)
}

/// Render a square album image centred in `area` using truecolor half-blocks.
/// True when the terminal supports 24-bit colour. The Linux text console does not,
/// and emitting RGB there both fails and corrupts later colours, so we fall back to
/// character-based ASCII art. MUSAKONE_ASCII=1 forces ASCII, =0 forces colour.
fn truecolor() -> bool {
    match std::env::var("MUSAKONE_ASCII").as_deref() {
        Ok("1") => return false,
        Ok("0") => return true,
        _ => {}
    }
    let ct = std::env::var("COLORTERM").unwrap_or_default();
    ct.contains("truecolor") || ct.contains("24bit")
}

const ASCII_RAMP: &[u8] = b" .,:;irsXA253hMHGS#9B&@";

/// Standard ANSI 16-colour palette (RGB) for quantising art on the Linux console.
const ANSI16: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (170, 0, 0),
    (0, 170, 0),
    (170, 85, 0),
    (0, 0, 170),
    (170, 0, 170),
    (0, 170, 170),
    (170, 170, 170),
    (85, 85, 85),
    (255, 85, 85),
    (85, 255, 85),
    (255, 255, 85),
    (85, 85, 255),
    (255, 85, 255),
    (85, 255, 255),
    (255, 255, 255),
];
// 4x4 Bayer matrix for ordered dithering (spreads quantisation error -> more apparent depth).
const BAYER4: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Nearest ANSI-16 colour index, with ordered dithering by pixel position.
fn quant16(p: [u8; 4], x: u32, y: u32) -> u8 {
    let d = (BAYER4[(y & 3) as usize][(x & 3) as usize] as f32 / 16.0 - 0.5) * 64.0;
    let ch = |c: u8| (c as f32 + d).clamp(0.0, 255.0) as i32;
    let (r, g, b) = (ch(p[0]), ch(p[1]), ch(p[2]));
    let mut best = 0u8;
    let mut bestd = i32::MAX;
    for (i, (pr, pg, pb)) in ANSI16.iter().enumerate() {
        let dist = (r - *pr as i32).pow(2) + (g - *pg as i32).pow(2) + (b - *pb as i32).pow(2);
        if dist < bestd {
            bestd = dist;
            best = i as u8;
        }
    }
    best
}

#[derive(PartialEq)]
enum ArtMode {
    True,
    Ansi16,
    Ascii,
}

fn art_mode() -> ArtMode {
    match std::env::var("MUSAKONE_ASCII").as_deref() {
        Ok("1") | Ok("ascii") => return ArtMode::Ascii,
        Ok("0") => return ArtMode::True,
        _ => {}
    }
    if truecolor() {
        ArtMode::True
    } else {
        // The Linux text console has 16 colours and the ▀ half-block; that beats mono ASCII.
        ArtMode::Ansi16
    }
}

fn render_cover(f: &mut Frame, area: Rect, img: &image::DynamicImage) {
    use image::GenericImageView;
    if area.width < 2 || area.height < 1 {
        return;
    }
    let mode = art_mode();
    let ascii = mode == ArtMode::Ascii;
    // A cell is ~twice as tall as wide. Half-block modes stack two pixels per cell
    // (cols = 2*rows); ASCII uses one pixel per cell but samples the same square.
    let rows = (area.height as u32).min(area.width as u32 / 2).max(1);
    let cols = rows * 2;
    let scaled = img.resize_exact(
        cols,
        if ascii { rows } else { rows * 2 },
        image::imageops::FilterType::Triangle,
    );
    let (cols_u, rows_u) = (cols as u16, rows as u16);
    let x0 = area.x + area.width.saturating_sub(cols_u) / 2;
    let y0 = area.y + area.height.saturating_sub(rows_u) / 2;
    let mut lines = Vec::with_capacity(rows as usize);
    for ry in 0..rows {
        let mut spans = Vec::with_capacity(cols as usize);
        for cx in 0..cols {
            match mode {
                ArtMode::True => {
                    let top = scaled.get_pixel(cx, ry * 2).0;
                    let bot = scaled.get_pixel(cx, ry * 2 + 1).0;
                    spans.push(Span::styled(
                        "▀",
                        Style::default()
                            .fg(Color::Rgb(top[0], top[1], top[2]))
                            .bg(Color::Rgb(bot[0], bot[1], bot[2])),
                    ));
                }
                ArtMode::Ansi16 => {
                    let top = quant16(scaled.get_pixel(cx, ry * 2).0, cx, ry * 2);
                    let bot = quant16(scaled.get_pixel(cx, ry * 2 + 1).0, cx, ry * 2 + 1);
                    spans.push(Span::styled(
                        "▀",
                        Style::default()
                            .fg(Color::Indexed(top))
                            .bg(Color::Indexed(bot)),
                    ));
                }
                ArtMode::Ascii => {
                    let p = scaled.get_pixel(cx, ry).0;
                    let luma = (p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000;
                    let idx = (luma as usize * (ASCII_RAMP.len() - 1)) / 255;
                    spans.push(Span::raw((ASCII_RAMP[idx] as char).to_string()));
                }
            }
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            x: x0,
            y: y0,
            width: cols_u,
            height: rows_u,
        },
    );
}

/// Stats screen: room or personal listening stats from the Gleam backend.
fn draw_stats(f: &mut Frame, area: Rect, app: &App) {
    use serde_json::Value;
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let window = match app.stats_days {
        7 => "7 days",
        30 => "30 days",
        _ => "all time",
    };
    let header = Line::from(vec![
        Span::styled(format!("{} · {window}", app.stats_scope), bold.fg(MAIN)),
        Span::styled("   (←/→ window · Space room/me · u refresh)", dim),
    ]);
    let Some(s) = &app.stats else {
        f.render_widget(
            Paragraph::new(vec![
                header,
                Line::raw(""),
                Line::styled("Counting...", dim),
            ]),
            area,
        );
        return;
    };

    let mut lines = vec![header, Line::raw("")];
    let sec = |lines: &mut Vec<Line>, t: &str| {
        lines.push(Line::raw(""));
        lines.push(Line::styled(t.to_string(), bold.fg(MAIN)));
    };
    let kv =
        |k: &str, v: String| Line::from(vec![Span::styled(format!("{k}: "), bold), Span::raw(v)]);
    let i = |v: &Value, p: &str| v.pointer(p).and_then(Value::as_i64).unwrap_or(0);
    let fnum = |v: &Value, p: &str| v.pointer(p).and_then(Value::as_f64).unwrap_or(0.0);
    let hrs = |h: f64| {
        if h >= 48.0 {
            format!("{:.0} d", h / 24.0)
        } else {
            format!("{h:.1} h")
        }
    };

    if app.stats_scope == "room" {
        sec(&mut lines, "Room");
        lines.push(kv("Plays", format!("{}", i(s, "/totals/plays"))));
        lines.push(kv("Heard", hrs(fnum(s, "/totals/hours"))));
        lines.push(kv(
            "Tracks / artists",
            format!("{} / {}", i(s, "/totals/tracks"), i(s, "/totals/artists")),
        ));
        lines.push(kv(
            "Play days / sessions",
            format!(
                "{} / {}",
                i(s, "/totals/play_days"),
                i(s, "/totals/sessions")
            ),
        ));
        lines.push(kv("Queue adds", format!("{}", i(s, "/totals/adds"))));
        lines.push(kv("Searches", format!("{}", i(s, "/totals/searches"))));

        sec(&mut lines, "Top tracks");
        for t in s
            .pointer("/top_tracks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(12)
        {
            let name = t.get("name").and_then(Value::as_str).unwrap_or("");
            let artist = t.get("artist").and_then(Value::as_str).unwrap_or("");
            lines.push(Line::from(vec![
                Span::raw(format!(
                    "{:>4}× ",
                    t.get("plays").and_then(Value::as_i64).unwrap_or(0)
                )),
                Span::raw(name.to_string()),
                Span::styled(format!("  {artist}"), dim),
            ]));
        }

        sec(&mut lines, "Top artists");
        for a in s
            .pointer("/top_artists")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(10)
        {
            lines.push(Line::raw(format!(
                "{:>4}×  {}",
                a.get("plays").and_then(Value::as_i64).unwrap_or(0),
                a.get("artist").and_then(Value::as_str).unwrap_or(""),
            )));
        }

        sec(&mut lines, "DJs");
        for d in s
            .pointer("/djs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(10)
        {
            lines.push(Line::from(vec![
                Span::raw(format!(
                    "{:<16}",
                    d.get("username").and_then(Value::as_str).unwrap_or("")
                )),
                Span::styled(
                    format!(
                        "{} added · {}% heard 24h · {} played",
                        d.get("tracks_added").and_then(Value::as_i64).unwrap_or(0),
                        d.get("heard_24h_pct").and_then(Value::as_i64).unwrap_or(0),
                        d.get("plays_attributed")
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                    ),
                    dim,
                ),
            ]));
        }

        sec(&mut lines, "Top searches");
        for q in s
            .pointer("/search/top_queries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(6)
        {
            lines.push(Line::from(vec![
                Span::styled("  ", dim),
                Span::raw(format!(
                    "{:<24}",
                    q.get("query").and_then(Value::as_str).unwrap_or("")
                )),
                Span::styled(
                    format!(
                        "{}× · {}% add",
                        q.get("searches").and_then(Value::as_i64).unwrap_or(0),
                        q.get("converted_pct").and_then(Value::as_i64).unwrap_or(0),
                    ),
                    dim,
                ),
            ]));
        }

        sec(&mut lines, "Decades");
        for d in s
            .pointer("/decades")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            lines.push(Line::raw(format!(
                "{:<6} {}",
                d.get("decade").and_then(Value::as_str).unwrap_or(""),
                d.get("plays").and_then(Value::as_i64).unwrap_or(0),
            )));
        }
    } else {
        sec(&mut lines, "You");
        lines.push(kv("Plays you caused", format!("{}", i(s, "/plays/plays"))));
        lines.push(kv("Heard", hrs(fnum(s, "/plays/hours"))));
        lines.push(kv(
            "Tracks queued",
            format!("{}", i(s, "/queue/tracks_added")),
        ));
        lines.push(kv(
            "Heard within 24h",
            format!("{}%", i(s, "/queue/heard_24h_pct")),
        ));
        lines.push(kv(
            "Share of room adds",
            format!("{}%", i(s, "/share_of_room_pct")),
        ));
        lines.push(kv(
            "DJ rank",
            format!("{} / {}", i(s, "/dj_rank"), i(s, "/dj_count")),
        ));

        sec(&mut lines, "Your top artists");
        for a in s
            .pointer("/top_artists")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(10)
        {
            lines.push(Line::raw(format!(
                "{:>4}×  {}",
                a.get("plays").and_then(Value::as_i64).unwrap_or(0),
                a.get("artist").and_then(Value::as_str).unwrap_or(""),
            )));
        }

        sec(&mut lines, "Your top tracks");
        for t in s
            .pointer("/top_tracks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(10)
        {
            lines.push(Line::from(vec![
                Span::raw(format!(
                    "{:>4}× ",
                    t.get("plays").and_then(Value::as_i64).unwrap_or(0)
                )),
                Span::raw(
                    t.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                ),
                Span::styled(
                    format!(
                        "  {}",
                        t.get("artist").and_then(Value::as_str).unwrap_or("")
                    ),
                    dim,
                ),
            ]));
        }

        sec(&mut lines, "Kindred (same artists queued)");
        for k in s
            .pointer("/kindred")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(6)
        {
            lines.push(Line::raw(format!(
                "{:<16} {} shared artists",
                k.get("username").and_then(Value::as_str).unwrap_or(""),
                k.get("shared_artists").and_then(Value::as_i64).unwrap_or(0),
            )));
        }
    }

    let max_off = lines.len().saturating_sub(area.height as usize);
    let off = app.stats_offset.min(max_off);
    let shown: Vec<Line> = lines
        .into_iter()
        .skip(off)
        .take(area.height as usize)
        .collect();
    f.render_widget(Paragraph::new(shown).style(Style::default().fg(MAIN)), area);
}

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
            ("6", "Show stats"),
            ("l", "Show/hide song lyrics"),
            ("[ ]", "Jump to previous / next album"),
            ("{ }", "Jump to previous / next artist"),
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
            ("Ctrl-V", "Select range (press at both ends)"),
            ("B", "Select whole album around the cursor"),
            ("Space", "Select current item"),
            ("a", "Add selected items to playlist"),
            ("i", "Show song info"),
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
