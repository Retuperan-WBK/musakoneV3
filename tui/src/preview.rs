//! Offline Mac/Linux PNG preview of the real framebuffer rendering path.
use crate::api::Api;
use crate::app::{App, LyricsState, Msg, PlayState, Screen};
use crate::model::{TlTrack, Track};
use crate::{fb, pixels, rpc::Rpc, ui};
use anyhow::{bail, Context, Result};
use image::{DynamicImage, Rgb, RgbImage};
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

struct Options {
    output: PathBuf,
    size: (u32, u32),
    cell: (u32, u32),
    text: String,
    cover: Option<PathBuf>,
    open: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            output: std::env::temp_dir().join("musakone-preview"),
            size: (1280, 800),
            cell: (8, 16),
            text: "Anna musiikin soida".into(),
            cover: None,
            open: false,
        }
    }
}

fn pair(value: &str) -> Result<(u32, u32)> {
    let (w, h) = value.split_once('x').context("Expected WIDTHxHEIGHT")?;
    Ok((w.parse()?, h.parse()?))
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--open" {
                options.open = true;
                continue;
            }
            let value = args
                .next()
                .with_context(|| format!("Missing value for {arg}"))?;
            match arg.as_str() {
                "--output" => options.output = value.into(),
                "--size" => options.size = pair(&value)?,
                "--cell" => options.cell = pair(&value)?,
                "--text" => options.text = value,
                "--cover" => options.cover = Some(value.into()),
                _ => bail!("Unknown preview option: {arg}"),
            }
        }
        let (w, h) = options.size;
        let (cw, ch) = options.cell;
        if !(320..=4096).contains(&w)
            || !(240..=2160).contains(&h)
            || !(4..=32).contains(&cw)
            || !(8..=64).contains(&ch)
            || w / cw < 20
            || h / ch < 10
        {
            bail!("Use a 320x240–4096x2160 image and sensible terminal cell dimensions");
        }
        if options.text.trim().is_empty() || options.text.chars().count() > 1024 {
            bail!("Preview text must contain 1–1024 characters");
        }
        Ok(options)
    }
}

fn color(value: Color, foreground: bool, bold: bool) -> [u8; 3] {
    let basic = match value {
        Color::Reset => return if foreground { [192; 3] } else { [0; 3] },
        Color::Rgb(r, g, b) => return [r, g, b],
        Color::Indexed(i) if i >= 232 => return [8 + (i - 232) * 10; 3],
        Color::Indexed(i) if i >= 16 => {
            let n = i - 16;
            let component = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
            return [component(n / 36), component(n / 6 % 6), component(n % 6)];
        }
        Color::Indexed(i) => i,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
    };
    pixels::ANSI16[(if bold && foreground && (1..8).contains(&basic) {
        basic + 8
    } else {
        basic
    }) as usize]
}

fn render(app: &mut App, screen: Screen, size: (u32, u32), cell: (u32, u32)) -> Result<RgbImage> {
    let columns = (size.0 / cell.0) as u16;
    let rows = (size.1 / cell.1) as u16;
    app.screen = screen;
    app.pixel_size = Some(size);
    let mut terminal = ratatui::Terminal::new(TestBackend::new(columns, rows))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    let mut image = RgbImage::new(size.0, size.1);
    let buffer = terminal.backend().buffer();
    let terminal_area = buffer.area;
    for y in 0..rows {
        let mut x = 0;
        while x < columns {
            let c = &buffer[(x, y)];
            let span = c.symbol().width().max(1) as u16;
            let area = ratatui::layout::Rect::new(x, y, span.min(columns - x), 1);
            let at =
                pixels::cell_area(size, terminal_area, area).context("Invalid preview geometry")?;
            let bold = c.modifier.contains(Modifier::BOLD);
            let (fg, bg) = if c.modifier.contains(Modifier::REVERSED) {
                (c.bg, c.fg)
            } else {
                (c.fg, c.bg)
            };
            pixels::fill(&mut image, at, color(bg, false, false));
            if !c.symbol().trim().is_empty() {
                if let Some(glyph) = pixels::text(
                    c.symbol(),
                    at.w,
                    at.h,
                    (cell.1 * 7 / 8) as u16,
                    color(fg, true, bold),
                ) {
                    fb::Blit::image(at.x, at.y, glyph).paint(&mut image);
                }
            }
            x += span;
        }
    }
    for overlay in app.fb_blits.borrow().iter() {
        overlay.paint(&mut image);
    }
    Ok(image)
}

fn demo(options: &Options) -> Result<App> {
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
    app.connected = true;
    app.state = PlayState::Paused;
    app.volume = 32;
    let uri = "preview:track".to_string();
    app.current = Some(TlTrack {
        tlid: 1,
        track: Track {
            uri: uri.clone(),
            name: "Offline framebuffer preview".into(),
            artists: "Musakone".into(),
            album: "Same lyric pixels on Mac and Ubuntu".into(),
            length: Some(240_000),
            ..Default::default()
        },
    });
    app.on_msg(Msg::Position(82_000));
    let cover = match &options.cover {
        Some(path) => {
            image::open(path).with_context(|| format!("Could not read cover {}", path.display()))?
        }
        None => DynamicImage::ImageRgb8(RgbImage::from_fn(600, 600, |x, y| {
            let ring = ((x as i32 - 300).pow(2) + (y as i32 - 300).pow(2)) / 1500 % 2;
            Rgb([
                30 + (x / 4) as u8,
                25 + (y / 5) as u8,
                if ring == 0 { 100 } else { 150 },
            ])
        })),
    };
    app.art_cache.insert(uri.clone(), Some(cover));
    app.lyrics = LyricsState::Loaded(
        uri,
        vec![
            (Some(0), "Previous lyric cue".into()),
            (Some(80_000), options.text.clone()),
            (Some(90_000), "Next lyric cue".into()),
            (Some(100_000), "Following lyric cue".into()),
        ],
    );
    Ok(app)
}

pub fn run(args: impl IntoIterator<Item = String>) -> Result<()> {
    let args: Vec<_> = args.into_iter().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("musakone preview [--output DIR] [--size 1280x800] [--cell 8x16] [--text TEXT] [--cover FILE] [--open]");
        println!("Offline PNG preview of the Linux framebuffer. Uses demo lyrics, never connects or plays music.");
        return Ok(());
    }
    let options = Options::parse(args)?;
    let mut app = demo(&options)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let output = options.output.join(format!("preview-{stamp}"));
    std::fs::create_dir_all(&options.output)?;
    std::fs::create_dir(&output)?;
    let mut paths = vec![];
    for (screen, name) in [
        (Screen::NowPlaying, "now-playing.png"),
        (Screen::Lyrics, "live-lyrics.png"),
    ] {
        let path = output.join(name);
        render(&mut app, screen, options.size, options.cell)?.save(&path)?;
        println!("{}", path.display());
        paths.push(path);
    }
    println!("Simulated framebuffer at {}x{}, {}x{} console cells. Demo lyrics only; no playback or configuration changes.", options.size.0, options.size.1, options.cell.0, options.cell.1);
    if options.open {
        for path in &paths {
            let command = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            if !std::process::Command::new(command)
                .arg(path)
                .status()?
                .success()
            {
                bail!("Preview saved, but could not open {}", path.display());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_blits_applied(app: &App, image: &RgbImage) {
        for blit in app.fb_blits.borrow().iter() {
            for y in 0..blit.h {
                for x in 0..blit.w {
                    let i = (y * blit.w + x) as usize * 3;
                    assert_eq!(
                        image.get_pixel(blit.x + x, blit.y + y).0,
                        blit.rgb[i..i + 3]
                    );
                }
            }
        }
    }

    fn assert_lyric_blit(blit: &fb::Blit, text: &str, px: u16, color: [u8; 3]) {
        assert_eq!(
            blit.rgb.as_ref(),
            pixels::text(text, blit.w, blit.h, px, color)
                .unwrap()
                .into_raw()
                .as_slice(),
            "wrong lyric text, font size or color for {text:?}"
        );
    }

    #[test]
    fn rejects_unsafe_preview_sizes_and_missing_option_values() {
        for args in [
            vec!["--size", "0x0"],
            vec!["--size", "99999x99999"],
            vec!["--text"],
            vec!["--cell", "0x16"],
            vec!["--text", ""],
        ] {
            assert!(Options::parse(args.into_iter().map(String::from)).is_err());
        }
    }

    #[test]
    fn both_previews_apply_the_real_cover_and_font_blits() {
        let options = Options {
            size: (960, 640),
            ..Default::default()
        };
        let mut app = demo(&options).unwrap();
        let now = render(&mut app, Screen::NowPlaying, options.size, options.cell).unwrap();
        assert_eq!(app.fb_blits.borrow().len(), 2);
        assert_blits_applied(&app, &now);
        assert_lyric_blit(&app.fb_blits.borrow()[1], &options.text, 28, [255, 255, 85]);
        let lyrics = render(&mut app, Screen::Lyrics, options.size, options.cell).unwrap();
        assert_blits_applied(&app, &lyrics);
        let blits = app.fb_blits.borrow();
        assert_eq!(blits.len(), 3);
        assert_lyric_blit(&blits[0], &options.text, 48, [255, 255, 85]);
        assert_lyric_blit(&blits[1], "Next lyric cue", 28, [170; 3]);
        assert_lyric_blit(&blits[2], "Following lyric cue", 28, [170; 3]);
        assert!(blits[0].y + blits[0].h < blits[1].y);
        assert!(blits[1].y + blits[1].h < blits[2].y);
        assert_eq!(now.dimensions(), options.size);
        assert_ne!(now, lyrics);
    }

    #[test]
    fn lyric_slots_stay_fixed_before_the_first_line_and_through_the_end() {
        let options = Options::default();
        let mut app = demo(&options).unwrap();
        app.lyrics = LyricsState::Loaded(
            "preview:track".into(),
            vec![
                (Some(10_000), "First line".into()),
                (Some(20_000), "".into()),
                (Some(25_000), "  ".into()),
                (Some(30_000), "Second line".into()),
                (Some(40_000), "Third line".into()),
            ],
        );
        app.on_msg(Msg::Position(15_000));
        render(&mut app, Screen::Lyrics, options.size, options.cell).unwrap();
        let slots: Vec<_> = app.fb_blits.borrow().iter().map(fb::Blit::area).collect();
        assert_eq!(slots.len(), 3);

        for (position, current, upcoming) in [
            (0, "♪", vec!["First line", "Second line"]),
            (15_000, "First line", vec!["Second line", "Third line"]),
            (20_000, "♪", vec!["Second line", "Third line"]),
            (25_000, "♪", vec!["Second line", "Third line"]),
            (35_000, "Second line", vec!["Third line"]),
            (45_000, "Third line", vec![]),
        ] {
            app.on_msg(Msg::Position(position));
            render(&mut app, Screen::Lyrics, options.size, options.cell).unwrap();
            let blits = app.fb_blits.borrow();
            // The bundled font lacks the music note; that placeholder stays in
            // the terminal layer rather than becoming a pixel overlay.
            let has_current = current != "♪";
            let offset = usize::from(has_current);
            assert_eq!(blits.len(), offset + upcoming.len());
            if has_current {
                assert_lyric_blit(&blits[0], current, 48, [255, 255, 85]);
                assert_eq!(blits[0].area(), slots[0]);
            }
            for (i, text) in upcoming.iter().enumerate() {
                assert_lyric_blit(&blits[i + offset], text, 28, [170; 3]);
                assert_eq!(
                    blits[i + offset].area(),
                    slots[i + 1],
                    "lyric slot moved at {position}ms"
                );
            }
        }
    }

    #[test]
    fn upcoming_lyrics_never_show_stale_or_untimed_text() {
        let options = Options::default();
        let mut app = demo(&options).unwrap();
        for lyrics in [
            LyricsState::Fetching("preview:track".into()),
            LyricsState::Failed("preview:track".into()),
            LyricsState::Loaded("preview:track".into(), vec![(None, "Plain lyric".into())]),
            LyricsState::Loaded(
                "old:track".into(),
                vec![(Some(90_000), "Stale lyric".into())],
            ),
        ] {
            app.lyrics = lyrics;
            render(&mut app, Screen::Lyrics, options.size, options.cell).unwrap();
            assert!(app.fb_blits.borrow().is_empty());
        }
        app.lyrics = LyricsState::Loaded(
            "preview:track".into(),
            vec![
                (Some(80_000), "Current line".into()),
                (Some(90_000), "Next timed line".into()),
                (None, "Untimed cue must stay hidden".into()),
            ],
        );
        render(&mut app, Screen::Lyrics, options.size, options.cell).unwrap();
        let blits = app.fb_blits.borrow();
        assert_eq!(blits.len(), 2);
        assert_lyric_blit(&blits[1], "Next timed line", 28, [170; 3]);
    }

    #[test]
    fn tab_two_keeps_every_pixel_outside_the_lyric_box_fixed() {
        let options = Options {
            size: (960, 640),
            ..Default::default()
        };
        let mut app = demo(&options).unwrap();
        let first = render(&mut app, Screen::NowPlaying, options.size, options.cell).unwrap();
        let area = app.fb_blits.borrow()[1].area();
        let cover = app.fb_blits.borrow()[0].area();
        for text in ["Short line", "A much longer lyric line that needs to wrap without moving the cover or the track information", ""] {
            app.lyrics = if text.is_empty() { LyricsState::Fetching("preview:track".into()) } else {
                LyricsState::Loaded("preview:track".into(), vec![(Some(0), text.into())])
            };
            let next = render(&mut app, Screen::NowPlaying, options.size, options.cell).unwrap();
            assert_eq!(app.fb_blits.borrow()[0].area(), cover);
            for (x, y, pixel) in first.enumerate_pixels() {
                if x < area.x || x >= area.x + area.w || y < area.y || y >= area.y + area.h {
                    assert_eq!(pixel, next.get_pixel(x, y), "layout moved outside lyric box at {x},{y}");
                }
            }
        }
    }
}
