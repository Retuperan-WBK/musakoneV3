//! Rendering, modelled on ncmpcpp's classic design with its default config:
//! header (bold title, " Volume: n%"), state line with [flags], main window in yellow,
//! cursor = yellow reverse (white reverse in inactive columns), progress bar "=>",
//! statusbar "Playing: song" + "[elapsed/total]".
use crate::app::*;
use crate::model::{fmt_long, fmt_ms, Track};
use crate::pixels::ANSI16;
use ratatui::layout::{Alignment, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
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

pub struct RenderCache {
    covers: crate::cache::Cache<CoverKey, CoverRender>,
    lyrics: crate::cache::Cache<LyricKey, Option<crate::fb::Blit>>,
}

impl Default for RenderCache {
    fn default() -> Self {
        Self {
            covers: crate::cache::Cache::new(4, 16 * 1024 * 1024),
            lyrics: crate::cache::Cache::new(8, 8 * 1024 * 1024),
        }
    }
}

impl RenderCache {
    pub fn clear_covers(&mut self) {
        self.covers.clear();
    }
}

#[derive(PartialEq)]
struct CoverKey {
    uri: String,
    width: u32,
    height: u32,
    mode: ArtMode,
}

#[derive(Clone)]
enum CoverRender {
    Pixels(crate::fb::Blit),
    Cells(Text<'static>),
}

#[derive(PartialEq)]
struct LyricKey {
    text: String,
    width: u32,
    height: u32,
    max_px: u16,
    color: [u8; 3],
}

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
    app.fb_blits.borrow_mut().clear();
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
    app.ensure_visible_metadata();

    draw_alt_header(f, np0, np1, divider, app);
    match app.screen {
        Screen::Help => draw_help(f, main, app),
        Screen::Playlist => draw_playlist(f, main, app),
        Screen::NowPlaying => draw_now_playing(f, main, app),
        Screen::Browser => draw_browser(f, main, app),
        Screen::Search => draw_search(f, main, app),
        Screen::Library => draw_library(f, main, app),
        Screen::Editor => draw_editor(f, main, app),
        Screen::Lyrics => draw_lyrics(f, main, app),
        Screen::Stats => draw_stats(f, main, app),
    }
    draw_progress(f, bar, app);
    draw_statusbar(f, status, app);
    if app.popup.is_some() || app.info.is_some() {
        // A main-window pixel overlay must never paint over a popup's text.
        app.fb_blits.borrow_mut().clear();
    }
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
                app,
                uri,
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
        Screen::NowPlaying => "Now playing".to_string(),
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
        Screen::Lyrics => "Live lyrics".into(),
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
            Span::styled(plain(&s.content), st)
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

/// Drop zero-width characters (combining marks, ZWJ, variation selectors). unicode-width
/// counts them as 0 but the Linux console draws each codepoint in its own cell, so a
/// title with them would run past its column and over the panel border.
fn plain(s: &str) -> String {
    s.chars().filter(|c| c.width().unwrap_or(0) > 0).collect()
}

fn pad(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if cw == 0 {
            continue; // console fonts give combining marks a cell of their own
        }
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
            // ncmpcpp (display.cpp setProperties): outside the playlist window a song
            // that is already queued is drawn bold (= bright on the console); everything
            // else keeps the plain column colours. Now-playing is not marked here.
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
        match app.art_cache.get(&t.uri) {
            Some(Some(img)) => render_cover(f, art_area, img, app, &t.uri),
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
                Span::raw(plain(v)),
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

// ---------- now playing / artwork ----------

fn draw_now_playing(f: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(current) = &app.current else {
        f.render_widget(
            Paragraph::new("Nothing playing. Choose a song from the playlist or library.")
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(MAIN)),
            Rect {
                y: area.y + area.height / 2,
                height: area.height - area.height / 2,
                ..area
            },
        );
        return;
    };
    let t = &current.track;
    let inner = area.inner(Margin::new(2, 1));
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let bold = Style::default().add_modifier(Modifier::BOLD);
    let title = if t.name.is_empty() {
        t.filename()
    } else {
        plain(&t.name)
    };
    let artist = if t.album_artist().is_empty() {
        "Unknown artist"
    } else {
        t.album_artist()
    };
    let album = if t.album.is_empty() {
        "Unknown album"
    } else {
        &t.album
    };
    let mut album = plain(album);
    if !t.date.is_empty() {
        album += &format!(" ({})", plain(&t.date));
    }
    let mut lines = vec![
        Line::styled(title, bold.fg(NOW)),
        Line::styled(plain(artist), bold.fg(Color::Yellow)),
        Line::styled(album, Style::default().fg(Color::Cyan)),
    ];
    if !t.artists.is_empty() && t.artists != t.album_artist() {
        lines.push(Line::styled(
            format!("Track artist: {}", plain(&t.artists)),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // One centered column at every terminal width: cover, details, current lyric.
    // Reserve the text first and let the square cover fill the remaining height.
    let column = Rect {
        x: inner.x + inner.width.saturating_sub(120) / 2,
        width: inner.width.min(120),
        ..inner
    };
    let metadata_rows: usize = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(column.width as usize))
        .sum();
    let metadata_h =
        (metadata_rows.saturating_add(1).min(u16::MAX as usize) as u16).min(column.height);
    let remaining = column.height.saturating_sub(metadata_h);
    // Fixed reservation: the cover and metadata never jump when a lyric changes,
    // wraps, loads or becomes unavailable.
    let lyric_h = remaining.min(5);
    let lyric_gap = u16::from(column.height > metadata_h + lyric_h && lyric_h > 0);
    let available_art = column
        .height
        .saturating_sub(metadata_h + lyric_h + lyric_gap);
    let art_h = available_art.saturating_sub(1).min(column.width / 2);
    let art_gap = u16::from(art_h > 0);
    let used_h = art_h + art_gap + metadata_h + lyric_gap + lyric_h;
    let top = column.y + column.height.saturating_sub(used_h) / 2;
    let art_area = Rect {
        y: top,
        height: art_h,
        ..column
    };
    if art_h > 0 {
        match app.art_cache.get(&t.uri) {
            Some(Some(img)) => render_cover(f, art_area, img, app, &t.uri),
            art => f.render_widget(
                Paragraph::new(if art.is_some() {
                    "(no album art)"
                } else {
                    "loading album art..."
                })
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
                Rect {
                    y: top + art_h / 2,
                    height: 1,
                    ..art_area
                },
            ),
        }
    }
    let metadata_y = top + art_h + art_gap;
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        Rect {
            y: metadata_y,
            height: metadata_h,
            ..column
        },
    );
    draw_current_lyric(
        f,
        Rect {
            y: metadata_y + metadata_h + lyric_gap,
            height: lyric_h,
            ..column
        },
        app,
        &t.uri,
        LyricRole::Caption,
    );
}

#[derive(Clone, Copy)]
enum LyricRole {
    Caption,
    Current,
    Upcoming,
}

fn current_lyric_text<'a>(app: &'a App, uri: &str) -> (&'a str, bool) {
    match &app.lyrics {
        LyricsState::Loaded(u, lines) if u == uri && lines.iter().any(|(ts, _)| ts.is_some()) => (
            app.current_lyric()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("♪"),
            true,
        ),
        LyricsState::Loaded(u, _) if u == uri => ("No synced lyrics available", false),
        LyricsState::Failed(u) if u == uri => ("Synced lyrics unavailable", false),
        _ => ("Looking for synced lyrics...", false),
    }
}

fn draw_current_lyric(f: &mut Frame, area: Rect, app: &App, uri: &str, role: LyricRole) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (text, synced) = current_lyric_text(app, uri);
    if synced {
        draw_lyric_line(f, area, app, text, role);
    } else {
        f.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}

fn draw_lyric_line(f: &mut Frame, area: Rect, app: &App, text: &str, role: LyricRole) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let text = plain(text);
    let highlighted = !matches!(role, LyricRole::Upcoming);
    let style = if highlighted {
        Style::default().fg(NOW).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    if let Some(size) = app.pixel_size {
        if let Some(pixels) = crate::pixels::cell_area(size, f.area(), area) {
            let cell_h = size.1 / f.area().height as u32;
            // 28px captions/upcoming lines and 48px main lyrics on an 8x16 console.
            let max_px = if matches!(role, LyricRole::Current) {
                cell_h * 3
            } else {
                cell_h * 7 / 4
            };
            let color = if highlighted {
                [255, 255, 85]
            } else {
                [170; 3]
            };
            let key = LyricKey {
                text: text.clone(),
                width: pixels.w,
                height: pixels.h,
                max_px: max_px.min(160) as u16,
                color,
            };
            let mut cache = app.render_cache.borrow_mut();
            let blit = match cache.lyrics.get(&key) {
                Some(cached) => cached,
                None => {
                    let blit = crate::pixels::text(&text, pixels.w, pixels.h, key.max_px, color)
                        .map(|image| crate::fb::Blit::image(0, 0, image));
                    let bytes = blit.as_ref().map_or(0, |b| b.rgb.len()) + text.len();
                    cache.lyrics.insert(key, blit.clone(), bytes);
                    blit
                }
            };
            if let Some(mut blit) = blit {
                blit.x = pixels.x;
                blit.y = pixels.y;
                f.render_widget(Clear, area);
                app.fb_blits.borrow_mut().push(blit);
                return;
            }
        }
    }
    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .style(style),
        area,
    );
}

const ASCII_RAMP: &[u8] = b" .,:;irsXA253hMHGS#9B&@";

// 4x4 Bayer matrix for ordered dithering (spreads quantisation error -> more apparent depth).
const BAYER4: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// One renderable cell on a 16-colour console: a glyph drawn in `fg` over `bg`.
/// The glyph's ink coverage blends the two colours, so the set of (fg, bg, glyph)
/// triples reaches far more apparent colours than the 16-entry palette alone.
struct Cell16 {
    fg: u8,
    bg: u8,
    glyph: &'static str,
    rgb: (f32, f32, f32),
}

/// Glyphs the Linux console font actually has (verified on the clubroom VM:
/// U+2588 full block and U+2591/U+2592 shades exist; the half-blocks do not),
/// with their approximate ink coverage.
const COVERAGE: [(f32, &str); 4] = [(0.0, " "), (0.25, "░"), (0.5, "▒"), (1.0, "█")];

fn cells16() -> &'static [Cell16] {
    static TABLE: std::sync::OnceLock<Vec<Cell16>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v = Vec::with_capacity(8 * 16 * COVERAGE.len());
        // Backgrounds: the console only honours the 8 dark colours for bg.
        for bg in 0..8u8 {
            let [br, bgc, bb] = ANSI16[bg as usize];
            for fg in 0..16u8 {
                let [fr, fgc, fb] = ANSI16[fg as usize];
                for (cov, glyph) in COVERAGE {
                    // Space ignores fg and full block ignores bg: only emit each once.
                    if (cov == 0.0 && fg != 0) || (cov == 1.0 && bg != 0) {
                        continue;
                    }
                    let mix = |b: u8, f: u8| b as f32 * (1.0 - cov) + f as f32 * cov;
                    v.push(Cell16 {
                        fg,
                        bg,
                        glyph,
                        rgb: (mix(br, fr), mix(bgc, fgc), mix(bb, fb)),
                    });
                }
            }
        }
        v
    })
}

/// Best console cell for a pixel: ordered-dither the colour, then pick the blend
/// nearest in a luma-weighted RGB distance.
fn best_cell16(p: [u8; 4], x: u32, y: u32) -> &'static Cell16 {
    let d = (BAYER4[(y & 3) as usize][(x & 3) as usize] as f32 / 16.0 - 0.5) * 24.0;
    let r = (p[0] as f32 + d).clamp(0.0, 255.0);
    let g = (p[1] as f32 + d).clamp(0.0, 255.0);
    let b = (p[2] as f32 + d).clamp(0.0, 255.0);
    let mut best = &cells16()[0];
    let mut bestd = f32::MAX;
    for c in cells16() {
        let (dr, dg, db) = (r - c.rgb.0, g - c.rgb.1, b - c.rgb.2);
        let dist = 2.0 * dr * dr + 4.0 * dg * dg + 3.0 * db * db;
        if dist < bestd {
            bestd = dist;
            best = c;
        }
    }
    best
}

#[derive(PartialEq)]
enum ArtMode {
    /// Real pixels straight into /dev/fb0 (Linux console with a writable framebuffer)
    Fb,
    True,
    Ansi16,
    Ascii,
}

fn art_mode(app: &App) -> ArtMode {
    if app.pixel_size.is_some() {
        return ArtMode::Fb;
    }
    match std::env::var("MUSAKONE_ASCII").as_deref() {
        Ok("1") | Ok("ascii") => return ArtMode::Ascii,
        Ok("0") => return ArtMode::True,
        _ => {}
    }
    let ct = std::env::var("COLORTERM").unwrap_or_default();
    if ct.contains("truecolor") || ct.contains("24bit") {
        ArtMode::True
    } else {
        // 16-colour console: blend fg/bg through the glyphs its font actually has.
        ArtMode::Ansi16
    }
}

/// Framebuffer path: map the cell rect to screen pixels, scale the cover to fill a
/// square inside it, record the blit, and leave the cells blank so text never lands
/// on top of the image.
fn render_cover_fb(f: &mut Frame, area: Rect, img: &image::DynamicImage, app: &App, uri: &str) {
    let Some(size) = app.pixel_size else { return };
    let Some(pixels) = crate::pixels::cell_area(size, f.area(), area) else {
        return;
    };
    let side = pixels.w.min(pixels.h);
    if side < 4 {
        return;
    }
    let x = pixels.x + (pixels.w - side) / 2;
    let y = pixels.y + (pixels.h - side) / 2;
    let key = CoverKey {
        uri: uri.into(),
        width: side,
        height: side,
        mode: ArtMode::Fb,
    };
    let mut cache = app.render_cache.borrow_mut();
    let mut blit = match cache.covers.get(&key) {
        Some(CoverRender::Pixels(blit)) => blit,
        _ => {
            let scaled = img
                .resize_exact(side, side, image::imageops::FilterType::Triangle)
                .to_rgb8();
            let blit = crate::fb::Blit::image(0, 0, scaled);
            cache
                .covers
                .insert(key, CoverRender::Pixels(blit.clone()), blit.rgb.len());
            blit
        }
    };
    blit.x = x;
    blit.y = y;
    app.fb_blits.borrow_mut().push(blit);
    // Blank the cells under the image (default black bg) so the text layer is empty there.
    f.render_widget(Clear, area);
}

fn render_cover(f: &mut Frame, area: Rect, img: &image::DynamicImage, app: &App, uri: &str) {
    use image::GenericImageView;
    if area.width < 2 || area.height < 1 {
        return;
    }
    let mode = art_mode(app);
    if mode == ArtMode::Fb {
        return render_cover_fb(f, area, img, app, uri);
    }
    // A cell is ~twice as tall as wide. Truecolor stacks two pixels per cell with a
    // half-block (cols = 2*rows, sample a square). The 16-colour and ASCII modes draw
    // one pixel per cell, so they sample cols x rows: a 2:1 image that the 1:2 cell
    // aspect stretches back into a square on screen.
    let rows = (area.height as u32).min(area.width as u32 / 2).max(1);
    let cols = rows * 2;
    let target = Rect {
        x: area.x + area.width.saturating_sub(cols as u16) / 2,
        y: area.y + area.height.saturating_sub(rows as u16) / 2,
        width: cols as u16,
        height: rows as u16,
    };
    let key = CoverKey {
        uri: uri.into(),
        width: cols,
        height: rows,
        mode,
    };
    let mut cache = app.render_cache.borrow_mut();
    if let Some(CoverRender::Cells(text)) = cache.covers.get(&key) {
        f.render_widget(Paragraph::new(text), target);
        return;
    }
    let mode = &key.mode;
    let scaled = img.resize_exact(
        cols,
        if *mode == ArtMode::True {
            rows * 2
        } else {
            rows
        },
        image::imageops::FilterType::Triangle,
    );
    let mut lines = Vec::with_capacity(rows as usize);
    for ry in 0..rows {
        let mut spans = Vec::with_capacity(cols as usize);
        for cx in 0..cols {
            match mode {
                // Fb returned early above; the text layer never reaches here.
                ArtMode::Fb => unreachable!("framebuffer mode handled by render_cover_fb"),
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
                    let c = best_cell16(scaled.get_pixel(cx, ry).0, cx, ry);
                    spans.push(Span::styled(
                        c.glyph,
                        Style::default()
                            .fg(Color::Indexed(c.fg))
                            .bg(Color::Indexed(c.bg)),
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
    let text = Text::from(lines);
    cache.covers.insert(
        key,
        CoverRender::Cells(text.clone()),
        cols as usize * rows as usize * std::mem::size_of::<Span>(),
    );
    f.render_widget(Paragraph::new(text), target);
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

fn draw_lyrics(f: &mut Frame, area: Rect, app: &App) {
    let inner = area.inner(Margin::new(2, 1));
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let Some(current) = &app.current else {
        f.render_widget(
            Paragraph::new("Nothing playing. Play a song to see live lyrics.")
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(MAIN)),
            Rect {
                y: inner.y + inner.height / 2,
                height: inner.height.div_ceil(2),
                ..inner
            },
        );
        return;
    };
    let uri = &current.track.uri;
    // Three fixed slots, independent of text length or how many cues remain.
    // In short windows give the current line priority over the upcoming lines.
    let cue_h = (inner.height / 4).min(5);
    let gap = u16::from(inner.height >= 9);
    let height = inner.height.saturating_sub(2 * (cue_h + gap)).min(10);
    let top = inner.y + (inner.height - height - 2 * (cue_h + gap)) / 2;
    draw_current_lyric(
        f,
        Rect {
            y: top,
            height,
            ..inner
        },
        app,
        uri,
        LyricRole::Current,
    );

    // Only the next two nonempty timed lines; never show a previous verse or
    // untimed text. Before the first timestamp, preview the first two lines.
    if cue_h > 0 {
        if let LyricsState::Loaded(loaded_uri, lines) = &app.lyrics {
            if loaded_uri == uri && lines.iter().any(|(ts, _)| ts.is_some()) {
                let active = app.lyrics_active();
                let next = lines
                    .iter()
                    .skip(active.map_or(0, |i| i + 1))
                    .filter(|(ts, text)| ts.is_some() && !text.trim().is_empty())
                    .take(2);
                for (i, (_, text)) in next.enumerate() {
                    draw_lyric_line(
                        f,
                        Rect {
                            y: top + height + gap + i as u16 * (cue_h + gap),
                            height: cue_h,
                            ..inner
                        },
                        app,
                        text,
                        LyricRole::Upcoming,
                    );
                }
            }
        }
    }
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
            ("Tab", "Switch between playlist and browser"),
            ("Shift-Tab", "Switch between playlist and browser"),
            ("F1", "Show help"),
            ("1", "Show playlist"),
            (
                "2",
                "Show now playing (cover / album artist / synced lyrics)",
            ),
            ("3", "Show search engine"),
            ("4", "Show browser"),
            ("5", "Show media library"),
            ("6", "Show playlist editor"),
            ("7", "Show stats"),
            ("8", "Show help / key bindings (also F1)"),
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
            ("Left -", "Decrease volume by 1% (unless Left navigates)"),
            ("Right +", "Increase volume by 1% (unless Right navigates)"),
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
        "Keys - Now playing",
        &[
            ("p", "Play / pause / resume"),
            ("< >", "Previous / next track"),
            ("l", "Show large live lyrics / return to now playing"),
            ("`", "Refetch lyrics"),
        ],
    ),
    (
        "Keys - Browser",
        &[
            ("Enter Right", "Enter directory"),
            ("Enter", "Add item to playlist and play it"),
            ("Space", "Add item to playlist"),
            ("Left Backspace", "Jump to parent directory"),
        ],
    ),
    (
        "Keys - Search engine",
        &[
            ("3", "Reset search and return cursor to Any"),
            ("Enter on Reset", "Clear search and return cursor to Any"),
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
        "Keys - Stats",
        &[
            ("Left Right", "Change time window (7d / 30d / all)"),
            ("Space Enter", "Toggle room / personal stats"),
            ("u", "Refresh stats"),
        ],
    ),
    (
        "Keys - Lyrics",
        &[
            ("l", "Return to the previous screen"),
            ("p", "Play / pause / resume (lyrics always follow playback)"),
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
