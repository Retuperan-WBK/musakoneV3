//! Rendering. ncmpcpp layout: header (title + flags/volume), separator, body,
//! progress bar, status line (state + song + time, or the prompt).
use crate::app::*;
use crate::model::fmt_ms;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const HELP: &[&str] = &[
    "Keys - Movement",
    "  Up k / Down j      Move cursor",
    "  PgUp PgDn ^U ^D    Page up / down",
    "  Home End G         First / last item",
    "  Tab / Shift-Tab    Next / previous screen",
    "  1 F1               Help",
    "  2                  Playlist (queue)",
    "  3                  Browse (Tidal, files)",
    "  4                  Search engine",
    "  5                  Mix (recommendations, autoplay)",
    "  6                  Playlist editor (your playlists)",
    "  l                  Lyrics (toggle)",
    "  q                  Quit",
    "",
    "Keys - Global",
    "  p                  Pause / resume",
    "  s                  Stop",
    "  > <                Next / previous track",
    "  f b                Seek forward / backward 5 s",
    "  g                  Jump to position (50%, 1:23, 83)",
    "  Right Left + -     Volume up / down",
    "  r z y R            Toggle repeat / random / single / consume",
    "  Z                  Shuffle queue",
    "  c                  Clear queue (asks)",
    "  C                  Crop queue (keep current)",
    "  o                  Go to current song",
    "  / . ,              Find in list, next, previous match",
    "  u                  Reload current screen",
    "  S                  Save queue as new playlist",
    "  N                  New empty playlist",
    "  A                  Toggle autoplay (backend fills the queue)",
    "  F                  Autoplay: add a batch now",
    "  L                  Log in as a different user",
    "",
    "Keys - Playlist (queue)",
    "  Enter              Play",
    "  d Delete           Remove from queue",
    "  m n                Move song up / down",
    "  a                  Add song to a playlist",
    "",
    "Keys - Browse",
    "  Enter              Open directory / add song and play",
    "  Space              Add item (whole album, artist or folder)",
    "  Backspace h        Parent directory",
    "  a                  Add song to a playlist",
    "",
    "Keys - Search engine",
    "  Enter on top row   Type the query",
    "  Enter              Add and play / open album or artist",
    "  Space              Add",
    "  a                  Add song to a playlist",
    "",
    "Keys - Mix",
    "  Enter / Space      Add recommendation and play / add",
    "  A F                Toggle autoplay / fill now",
    "  u                  Refresh recommendations",
    "",
    "Keys - Playlist editor",
    "  Left h / Right l   Playlists column / tracks column",
    "  Enter / Space      Add playlist or track and play / add",
    "  d                  Delete playlist / remove track",
    "  e                  Rename playlist",
    "  m n                Move track up / down",
    "",
    "Keys - Lyrics",
    "  j k                Scroll (stops following the song)",
    "  o                  Follow the song again",
    "  u                  Re-fetch lyrics",
];

const HEADER: Color = Color::Yellow;
const DIR: Color = Color::Blue;
const NOW: Color = Color::Yellow;
const VOL: Color = Color::Green;
const BAR: Color = Color::Green;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.height < 5 {
        return;
    }
    let [head, sep, body, bar, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    header(f, head, app);
    f.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).fg(Color::DarkGray),
        sep,
    );
    match app.view {
        View::Help => help(f, body, app),
        View::Queue => queue(f, body, app),
        View::Browser => browser(f, body, app),
        View::Search => search(f, body, app),
        View::Mix => mix(f, body, app),
        View::Playlists => playlists(f, body, app),
        View::Lyrics => lyrics(f, body, app),
    }
    progress(f, bar, app);
    statusline(f, status, app);
    if let Some(p) = &app.popup {
        popup(f, area, p);
    }
}

fn header(f: &mut Frame, area: Rect, app: &App) {
    let mut title = app.view.title().to_string();
    if app.view == View::Browser {
        title = format!("{}: {}", title, app.browse_path());
    }
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
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!("[{flags}] ")
    };
    let conn = if !app.connected {
        " [disconnected]"
    } else if !app.mopidy_ok {
        " [no mopidy]"
    } else {
        ""
    };
    let right = Line::from(vec![
        Span::raw(flags).fg(Color::Red),
        Span::raw("Volume: ").fg(VOL),
        Span::raw(format!("{}%", app.volume)).fg(VOL).bold(),
    ]);
    let rw = right.width() as u16;
    let left = Line::from(vec![
        Span::raw(title).fg(HEADER).bold(),
        Span::raw(conn).fg(Color::Red),
    ]);
    f.render_widget(
        Paragraph::new(left),
        Rect {
            width: area.width.saturating_sub(rw + 1),
            ..area
        },
    );
    f.render_widget(
        Paragraph::new(right).right_aligned(),
        Rect {
            x: area.x + area.width.saturating_sub(rw),
            width: rw,
            ..area
        },
    );
}

/// A row: left text, right text (duration), style.
pub struct Row<'a> {
    pub left: Line<'a>,
    pub right: String,
    pub style: Style,
}

/// Render only the visible slice, scrolling to keep the cursor on screen.
fn list<'a>(
    f: &mut Frame,
    area: Rect,
    len: usize,
    st: &mut ListState,
    focused: bool,
    mut row: impl FnMut(usize) -> Row<'a>,
) {
    let h = area.height as usize;
    st.height = h;
    if h == 0 {
        return;
    }
    if st.cursor >= len {
        st.cursor = len.saturating_sub(1);
    }
    // Keep cursor centred-ish: offset so cursor is visible with context.
    let offset = if len <= h {
        0
    } else {
        st.cursor.saturating_sub(h / 2).min(len - h)
    };
    let mut lines = Vec::with_capacity(h);
    for i in offset..(offset + h).min(len) {
        let r = row(i);
        let w = area.width as usize;
        let rw = r.right.width();
        let lw = if rw > 0 { w.saturating_sub(rw + 1) } else { w };
        let mut spans: Vec<Span> = truncate_line(r.left, lw);
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        let pad = w.saturating_sub(used + rw);
        spans.push(Span::raw(" ".repeat(pad)));
        if rw > 0 {
            spans.push(Span::raw(r.right));
        }
        let mut style = r.style;
        if focused && i == st.cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        lines.push(Line::from(spans).style(style));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn truncate_line(line: Line<'_>, width: usize) -> Vec<Span<'_>> {
    let mut out = vec![];
    let mut used = 0;
    for span in line.spans {
        let w = span.content.width();
        if used + w <= width {
            used += w;
            out.push(span);
            continue;
        }
        let mut s = String::new();
        for c in span.content.chars() {
            let cw = c.width().unwrap_or(0);
            if used + cw > width.saturating_sub(1) {
                break;
            }
            used += cw;
            s.push(c);
        }
        s.push('…');
        out.push(Span::styled(s, span.style));
        break;
    }
    out
}

fn queue(f: &mut Frame, area: Rect, app: &mut App) {
    if app.queue.is_empty() {
        return empty(
            f,
            area,
            if app.connected {
                "Playlist is empty"
            } else {
                "Connecting…"
            },
        );
    }
    let cur = app.current_tlid();
    let queue = std::mem::take(&mut app.queue);
    list(f, area, queue.len(), &mut app.queue_list, true, |i| {
        let t = &queue[i];
        let now = Some(t.tlid) == cur;
        Row {
            left: Line::raw(t.track.title()),
            right: t.track.duration(),
            style: if now {
                Style::default().fg(NOW).bold()
            } else {
                Style::default()
            },
        }
    });
    app.queue = queue;
}

fn browser(f: &mut Frame, area: Rect, app: &mut App) {
    let n = app.browse_rows();
    if n == 0 {
        return empty(
            f,
            area,
            if app.browse_loading {
                "Loading…"
            } else {
                "Empty"
            },
        );
    }
    let off = usize::from(app.browse_stack.len() > 1);
    let items = std::mem::take(&mut app.browse_items);
    let cur_uri = app.current.as_ref().map(|c| c.track.uri.clone());
    let mut st = app.browse_list;
    list(f, area, n, &mut st, true, |i| {
        if i < off {
            return Row {
                left: Line::raw(".."),
                right: String::new(),
                style: Style::default().fg(DIR).bold(),
            };
        }
        let r = &items[i - off];
        if r.is_track() {
            let (left, right) = match app.track_of(&r.uri) {
                Some(t) => (t.title(), t.duration()),
                None => (r.name.clone(), String::new()),
            };
            let now = cur_uri.as_deref() == Some(&r.uri);
            Row {
                left: Line::raw(left),
                right,
                style: if now {
                    Style::default().fg(NOW).bold()
                } else {
                    Style::default()
                },
            }
        } else {
            Row {
                left: Line::raw(format!("[{}]", r.name)),
                right: String::new(),
                style: Style::default().fg(DIR).bold(),
            }
        }
    });
    app.browse_items = items;
    app.browse_list = st;
}

fn search(f: &mut Frame, area: Rect, app: &mut App) {
    let rows = std::mem::take(&mut app.search_rows);
    let q = app.search_query.clone();
    let searching = app.searching;
    list(f, area, rows.len() + 1, &mut app.search_list, true, |i| {
        if i == 0 {
            let hint = if searching {
                "  (searching…)"
            } else if rows.is_empty() {
                "  (Enter to type)"
            } else {
                ""
            };
            return Row {
                left: Line::from(vec![
                    Span::raw("Any: ").fg(HEADER).bold(),
                    Span::raw(q.clone()),
                    Span::raw(hint).fg(Color::DarkGray),
                ]),
                right: String::new(),
                style: Style::default(),
            };
        }
        match &rows[i - 1] {
            SearchRow::Header(h) => Row {
                left: Line::raw(h.clone()),
                right: String::new(),
                style: Style::default().fg(HEADER).bold(),
            },
            SearchRow::Track(t) => Row {
                left: Line::raw(t.title()),
                right: t.duration(),
                style: Style::default(),
            },
            SearchRow::Ref(r) => Row {
                left: Line::raw(format!("[{}]", r.name)),
                right: String::new(),
                style: Style::default().fg(DIR).bold(),
            },
        }
    });
    app.search_rows = rows;
}

fn mix(f: &mut Frame, area: Rect, app: &mut App) {
    let [top, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    let ap = match &app.autoplay {
        Some(a) => format!(
            "Autoplay: {}  mode {}  keep {} ahead, batches of {}  discovery {:.0}%  added last 24h: {}",
            if a.settings.enabled { "ON" } else { "off" },
            a.settings.mode,
            a.settings.min_ahead,
            a.settings.batch_size,
            a.settings.discovery * 100.0,
            a.added_24h
        ),
        None => "Autoplay: …".into(),
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::raw(ap).fg(VOL),
            Line::raw("Recommendations  (A toggle autoplay, F fill now, u refresh)")
                .fg(Color::DarkGray),
        ]),
        top,
    );
    if app.recs.is_empty() {
        return empty(
            f,
            body,
            if app.recs_loading {
                "Asking the backend… (first time can take a while)"
            } else {
                "Press Enter to load recommendations"
            },
        );
    }
    let recs = std::mem::take(&mut app.recs);
    list(f, body, recs.len(), &mut app.rec_list, true, |i| {
        let r = &recs[i];
        let why = r.reasons.join(", ");
        Row {
            left: Line::from(vec![
                Span::raw(format!("{} - {}", r.artist, r.name)),
                Span::raw(if why.is_empty() {
                    String::new()
                } else {
                    format!("  ({why})")
                })
                .fg(Color::DarkGray),
            ]),
            right: r.duration_ms.map(fmt_ms).unwrap_or_default(),
            style: Style::default(),
        }
    });
    app.recs = recs;
}

fn playlists(f: &mut Frame, area: Rect, app: &mut App) {
    let [left, sep, right] = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new(vec![Line::raw("│"); area.height as usize]).fg(Color::DarkGray),
        sep,
    );
    if app.playlists.is_empty() {
        empty(
            f,
            left,
            if app.user.is_none() {
                "Log in (L) to see playlists"
            } else {
                "No playlists (N: new, S: save queue)"
            },
        );
    } else {
        let pls = std::mem::take(&mut app.playlists);
        let me = app.user.as_ref().and_then(|u| u.parse::<i64>().ok());
        let focused = !app.pl_right;
        list(f, left, pls.len(), &mut app.pl_list, focused, |i| {
            let p = &pls[i];
            let mine = me.is_none_or(|m| m == p.user_id);
            let tag = if !mine {
                " (shared)"
            } else if p.is_public {
                " (public)"
            } else {
                ""
            };
            Row {
                left: Line::from(vec![
                    Span::raw(p.name.clone()),
                    Span::raw(tag).fg(Color::DarkGray),
                ]),
                right: String::new(),
                style: Style::default().fg(DIR).bold(),
            }
        });
        app.playlists = pls;
    }
    if app.pl_tracks.is_empty() {
        return empty(
            f,
            right,
            if app.playlists.is_empty() {
                ""
            } else {
                "Empty playlist (a on a song adds it here)"
            },
        );
    }
    let uris = std::mem::take(&mut app.pl_tracks);
    let focused = app.pl_right;
    let cur_uri = app.current.as_ref().map(|c| c.track.uri.clone());
    let mut st = app.plt_list;
    list(f, right, uris.len(), &mut st, focused, |i| {
        let u = &uris[i];
        let (left, dur) = match app.track_of(u) {
            Some(t) => (t.title(), t.duration()),
            None => (u.clone(), String::new()),
        };
        let now = cur_uri.as_deref() == Some(u.as_str());
        Row {
            left: Line::raw(left),
            right: dur,
            style: if now {
                Style::default().fg(NOW).bold()
            } else {
                Style::default()
            },
        }
    });
    app.pl_tracks = uris;
    app.plt_list = st;
}

fn lyrics(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(cur) = app.current.clone() else {
        return empty(f, area, "Nothing playing");
    };
    let Some((uri, lines)) = app.lyrics.clone() else {
        return empty(
            f,
            area,
            &format!("Fetching lyrics for {}…", cur.track.title()),
        );
    };
    if uri != cur.track.uri {
        return empty(
            f,
            area,
            &format!("Fetching lyrics for {}…", cur.track.title()),
        );
    }
    let [top, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    let synced = lines.first().is_some_and(|l| l.0.is_some());
    f.render_widget(
        Paragraph::new(vec![
            Line::raw(cur.track.title()).fg(HEADER).bold(),
            Line::raw(if synced {
                "synced lyrics (lrclib.net)"
            } else {
                "lyrics (lrclib.net)"
            })
            .fg(Color::DarkGray),
        ]),
        top,
    );
    let active = app.lyrics_active();
    if app.lyrics_follow {
        if let Some(a) = active {
            app.lyrics_list.cursor = a;
        }
    }
    let focused = !app.lyrics_follow;
    list(f, body, lines.len(), &mut app.lyrics_list, focused, |i| {
        let style = if Some(i) == active {
            Style::default().fg(NOW).bold()
        } else if active.is_some_and(|a| i < a) {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
        };
        Row {
            left: Line::raw(lines[i].1.clone()),
            right: String::new(),
            style,
        }
    });
}

fn help(f: &mut Frame, area: Rect, app: &mut App) {
    list(f, area, HELP.len(), &mut app.help_list, false, |i| {
        let s = HELP[i];
        let style = if !s.starts_with(' ') && !s.is_empty() {
            Style::default().fg(HEADER).bold()
        } else {
            Style::default()
        };
        Row {
            left: Line::raw(s),
            right: String::new(),
            style,
        }
    });
}

fn empty(f: &mut Frame, area: Rect, msg: &str) {
    f.render_widget(Paragraph::new(msg).fg(Color::DarkGray), area);
}

fn progress(f: &mut Frame, area: Rect, app: &App) {
    let w = area.width as usize;
    let Some(len) = app
        .current
        .as_ref()
        .and_then(|c| c.track.length)
        .filter(|l| *l > 0)
    else {
        return f.render_widget(Paragraph::new("─".repeat(w)).fg(Color::DarkGray), area);
    };
    let frac = (app.elapsed_ms() as f64 / len as f64).clamp(0.0, 1.0);
    let filled = (frac * w as f64) as usize;
    let mut s = "=".repeat(filled.saturating_sub(1));
    if filled > 0 {
        s.push('>');
    }
    s.push_str(&"─".repeat(w.saturating_sub(filled)));
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(
                s[..s
                    .char_indices()
                    .nth(filled)
                    .map(|(i, _)| i)
                    .unwrap_or(s.len())]
                    .to_string(),
            )
            .fg(BAR)
            .bold(),
            Span::raw(
                s[s.char_indices()
                    .nth(filled)
                    .map(|(i, _)| i)
                    .unwrap_or(s.len())..]
                    .to_string(),
            )
            .fg(Color::DarkGray),
        ])),
        area,
    );
}

fn statusline(f: &mut Frame, area: Rect, app: &App) {
    if let Some(p) = &app.prompt {
        let shown = if p.masked {
            "*".repeat(p.buf.chars().count())
        } else {
            p.buf.clone()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(p.label.clone()).fg(HEADER).bold(),
                Span::raw(shown),
            ])),
            area,
        );
        let x = area.x
            + (p.label.width()
                + if p.masked {
                    p.buf.chars().count()
                } else {
                    p.buf.width()
                }) as u16;
        f.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
        return;
    }
    if let Some((m, _, err)) = &app.message {
        let style = if *err {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(VOL)
        };
        return f.render_widget(Paragraph::new(m.as_str()).style(style), area);
    }
    let (state, style) = match app.state {
        PlayState::Playing => ("Playing: ", Style::default().fg(VOL).bold()),
        PlayState::Paused => ("Paused: ", Style::default().fg(HEADER).bold()),
        PlayState::Stopped => ("", Style::default()),
    };
    let Some(cur) = &app.current else {
        return f.render_widget(Paragraph::new("Stopped").fg(Color::DarkGray), area);
    };
    let time = match cur.track.length {
        Some(l) => format!("[{}/{}]", fmt_ms(app.elapsed_ms().min(l)), fmt_ms(l)),
        None => format!("[{}]", fmt_ms(app.elapsed_ms())),
    };
    let tw = time.width() as u16;
    let song = Line::from(vec![
        Span::styled(state, style),
        Span::raw(cur.track.title()).bold(),
    ]);
    let song = Line::from(truncate_line(
        song,
        area.width.saturating_sub(tw + 1) as usize,
    ));
    f.render_widget(Paragraph::new(song), area);
    f.render_widget(
        Paragraph::new(time).right_aligned(),
        Rect {
            x: area.x + area.width.saturating_sub(tw),
            width: tw,
            ..area
        },
    );
}

fn popup(f: &mut Frame, area: Rect, p: &Popup) {
    let h = (p.items.len() as u16 + 2)
        .min(area.height.saturating_sub(4))
        .max(3);
    let w = (p.items.iter().map(|(_, n)| n.width()).max().unwrap_or(10) as u16 + 4)
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
        .fg(HEADER);
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
        .map(|(i, (_, n))| {
            let l = Line::raw(format!(" {n}"));
            if i == p.cursor {
                l.add_modifier(Modifier::REVERSED)
            } else {
                l
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}
