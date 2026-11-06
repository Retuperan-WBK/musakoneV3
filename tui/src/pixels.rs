//! Shared pixel geometry and antialiased TrueType text for framebuffer and PNG preview.
use fontdue::layout::{CoordinateSystem, HorizontalAlign, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};
use image::{Rgb, RgbImage};
use ratatui::layout::Rect;
use std::sync::OnceLock;

pub const FONT_LICENSE: &str = include_str!("../assets/fonts/OFL.txt");

/// Console palette shared by artwork quantization and the offline preview.
pub const ANSI16: [[u8; 3]; 16] = [
    [0, 0, 0],
    [170, 0, 0],
    [0, 170, 0],
    [170, 85, 0],
    [0, 0, 170],
    [170, 0, 170],
    [0, 170, 170],
    [170, 170, 170],
    [85, 85, 85],
    [255, 85, 85],
    [85, 255, 85],
    [255, 255, 85],
    [85, 85, 255],
    [255, 85, 255],
    [85, 255, 255],
    [255, 255, 255],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelArea {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub fn cell_area(size: (u32, u32), terminal: Rect, area: Rect) -> Option<PixelArea> {
    if terminal.width == 0 || terminal.height == 0 || area.width == 0 || area.height == 0 {
        return None;
    }
    let (cw, ch) = (
        size.0 / terminal.width as u32,
        size.1 / terminal.height as u32,
    );
    if cw == 0
        || ch == 0
        || area.x < terminal.x
        || area.y < terminal.y
        || area.right() > terminal.right()
        || area.bottom() > terminal.bottom()
    {
        return None;
    }
    Some(PixelArea {
        x: (area.x - terminal.x) as u32 * cw,
        y: (area.y - terminal.y) as u32 * ch,
        w: area.width as u32 * cw,
        h: area.height as u32 * ch,
    })
}

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::from_bytes(
            include_bytes!("../assets/fonts/NotoSansMono-Regular.ttf").as_slice(),
            FontSettings::default(),
        )
        .expect("bundled Noto Sans Mono font")
    })
}

/// Render the complete line, reducing size only when wrapping would overflow.
/// No OS fonts, graphics APIs or framebuffer device are needed for this function.
pub fn text(text: &str, width: u32, height: u32, max_px: u16, color: [u8; 3]) -> Option<RgbImage> {
    if width == 0
        || height == 0
        || width as u64 * height as u64 > 16_000_000
        || text.trim().is_empty()
        || text.chars().count() > 1024
    {
        return None;
    }
    let font = font();
    if text
        .chars()
        .any(|c| !c.is_whitespace() && font.lookup_glyph_index(c) == 0)
    {
        return None;
    }
    let padding = if width >= 40 && height >= 24 { 2 } else { 0 };
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    for px in (8..=max_px.min(160)).rev() {
        layout.reset(&LayoutSettings {
            max_width: Some(width.saturating_sub(2 * padding) as f32),
            horizontal_align: HorizontalAlign::Center,
            line_height: 1.05,
            ..Default::default()
        });
        layout.append(&[font], &TextStyle::new(text, px as f32, 0));
        let glyphs: Vec<_> = layout
            .glyphs()
            .iter()
            .filter(|g| g.width > 0 && g.height > 0)
            .collect();
        if glyphs.is_empty() {
            return None;
        }
        let left = glyphs.iter().map(|g| g.x as i32).min()?;
        let top = glyphs.iter().map(|g| g.y as i32).min()?;
        let right = glyphs.iter().map(|g| g.x as i32 + g.width as i32).max()?;
        let bottom = glyphs.iter().map(|g| g.y as i32 + g.height as i32).max()?;
        let ink_w = (right - left) as u32;
        let ink_h = (bottom - top) as u32;
        if ink_w > width.saturating_sub(2 * padding) || ink_h > height.saturating_sub(2 * padding) {
            continue;
        }
        let dx = (width - ink_w) as i32 / 2 - left;
        let dy = (height - ink_h) as i32 / 2 - top;
        let mut image = RgbImage::new(width, height);
        for g in glyphs {
            let (_, coverage) = font.rasterize_config(g.key);
            for y in 0..g.height {
                for x in 0..g.width {
                    let alpha = coverage[y * g.width + x] as u16;
                    if alpha == 0 {
                        continue;
                    }
                    let at = (
                        (g.x as i32 + dx + x as i32) as u32,
                        (g.y as i32 + dy + y as i32) as u32,
                    );
                    let pixel = image.get_pixel_mut(at.0, at.1);
                    for c in 0..3 {
                        pixel[c] = ((color[c] as u16 * alpha + pixel[c] as u16 * (255 - alpha))
                            / 255) as u8;
                    }
                }
            }
        }
        return Some(image);
    }
    None
}

pub fn fill(image: &mut RgbImage, area: PixelArea, color: [u8; 3]) {
    for y in area.y..area.y.saturating_add(area.h).min(image.height()) {
        for x in area.x..area.x.saturating_add(area.w).min(image.width()) {
            image.put_pixel(x, y, Rgb(color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_terminal_cells_to_the_same_pixels_on_every_platform() {
        assert_eq!(
            cell_area(
                (1280, 800),
                Rect::new(0, 0, 160, 50),
                Rect::new(10, 20, 100, 5)
            ),
            Some(PixelArea {
                x: 80,
                y: 320,
                w: 800,
                h: 80
            })
        );
        assert!(cell_area((10, 10), Rect::new(0, 0, 160, 50), Rect::new(0, 0, 1, 1)).is_none());
        assert!(cell_area(
            (1280, 800),
            Rect::new(0, 0, 160, 50),
            Rect::new(159, 0, 2, 1)
        )
        .is_none());
    }

    #[test]
    fn real_font_is_antialiased_and_preserves_case_and_nordic_text() {
        let image = text("Hyvää yötä, Åbo!", 600, 80, 32, [255, 255, 85]).unwrap();
        assert_eq!(image.dimensions(), (600, 80));
        assert!(image.pixels().any(|p| p[0] > 0 && p[0] < 255));
        assert_ne!(
            image,
            text("HYVÄÄ YÖTÄ, ÅBO!", 600, 80, 32, [255, 255, 85]).unwrap()
        );
    }

    #[test]
    fn long_lines_fit_without_clipping_and_missing_glyphs_fall_back() {
        let image = text(
            "A longer current lyric line that wraps onto more than one line",
            240,
            96,
            32,
            [255; 3],
        )
        .unwrap();
        assert!(image.pixels().any(|p| p[0] != 0));
        assert!(image
            .enumerate_pixels()
            .all(|(x, y, p)| p[0] == 0 || (x >= 2 && y >= 2 && x < 238 && y < 94)));
        assert!(text("你好", 600, 80, 32, [255; 3]).is_none());
        assert!(text("Hello", 0, 0, 32, [255; 3]).is_none());
    }
}
