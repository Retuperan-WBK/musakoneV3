//! Linux framebuffer blitting. On the text console (fbcon) we can draw real pixels
//! straight into /dev/fb0, so album art renders at full resolution and colour instead
//! of being squeezed through the 16-colour character grid. Geometry comes from sysfs,
//! so no ioctl/libc is needed. Only active when the device is writable (user in the
//! `video` group) and we are on the console (TERM=linux).
use crate::pixels::PixelArea;
use image::RgbImage;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::sync::OnceLock;

pub struct Fb {
    file: File,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bpp: u32,
}

/// A pending blit recorded during a draw and applied after the text layer flushes.
#[derive(Clone)]
pub struct Blit {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// RGB triples, row-major, w*h*3 bytes
    pub rgb: std::sync::Arc<[u8]>,
}

impl Blit {
    pub fn image(x: u32, y: u32, image: RgbImage) -> Self {
        Self {
            x,
            y,
            w: image.width(),
            h: image.height(),
            rgb: image.into_raw().into(),
        }
    }

    pub fn area(&self) -> PixelArea {
        PixelArea {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
        }
    }

    pub fn unchanged(&self, previous: &[Blit]) -> bool {
        previous
            .iter()
            .any(|old| old.area() == self.area() && std::sync::Arc::ptr_eq(&old.rgb, &self.rgb))
    }

    /// The preview applies the exact RGB bytes that the framebuffer receives.
    pub fn paint(&self, image: &mut RgbImage) {
        for y in 0..self.h.min(image.height().saturating_sub(self.y)) {
            for x in 0..self.w.min(image.width().saturating_sub(self.x)) {
                let i = (y as usize * self.w as usize + x as usize) * 3;
                image.put_pixel(
                    self.x + x,
                    self.y + y,
                    image::Rgb([self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]),
                );
            }
        }
    }
}

pub fn dimensions() -> Option<(u32, u32)> {
    if matches!(
        std::env::var("MUSAKONE_ASCII").as_deref(),
        Ok("0" | "1" | "ascii")
    ) {
        return None;
    }
    get().map(|fb| (fb.width, fb.height))
}

pub fn damaged_areas(previous: &[PixelArea], next: &[Blit]) -> Vec<PixelArea> {
    previous
        .iter()
        .copied()
        .filter(|area| !next.iter().any(|b| b.area() == *area))
        .collect()
}

pub fn damaged_rows(areas: &[PixelArea], pixels_high: u32, rows: u16) -> Vec<u16> {
    if rows == 0 || pixels_high < rows as u32 {
        return vec![];
    }
    let cell_h = pixels_high / rows as u32;
    let mut damaged = std::collections::BTreeSet::new();
    for area in areas {
        let end = area
            .y
            .saturating_add(area.h)
            .div_ceil(cell_h)
            .min(rows as u32);
        for y in (area.y / cell_h).min(end)..end {
            damaged.insert(y as u16);
        }
    }
    damaged.into_iter().collect()
}

fn read_sys(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/graphics/fb0/{name}")).ok()
}

/// Opens the framebuffer once. None when absent, unwritable, or not the console.
pub fn get() -> Option<&'static Fb> {
    static FB: OnceLock<Option<Fb>> = OnceLock::new();
    FB.get_or_init(|| {
        if std::env::var("MUSAKONE_FB").as_deref() == Ok("0") {
            return None;
        }
        // Only the real console draws through fbcon; an xterm/ssh session does not.
        let on_console = std::env::var("TERM").map(|t| t == "linux").unwrap_or(false)
            || std::env::var("MUSAKONE_FB").as_deref() == Ok("1");
        if !on_console {
            return None;
        }
        let file = OpenOptions::new().write(true).open("/dev/fb0").ok()?;
        let size = read_sys("virtual_size")?;
        let (w, h) = size.trim().split_once(',')?;
        let width: u32 = w.trim().parse().ok()?;
        let height: u32 = h.trim().parse().ok()?;
        let bpp: u32 = read_sys("bits_per_pixel")?.trim().parse().ok()?;
        let stride: u32 = read_sys("stride")?.trim().parse().ok()?;
        if bpp != 32 && bpp != 16 {
            return None;
        }
        Some(Fb {
            file,
            width,
            height,
            stride,
            bpp,
        })
    })
    .as_ref()
}

impl Fb {
    pub fn clear(&self, area: PixelArea) {
        let w = area.w.min(self.width.saturating_sub(area.x));
        let h = area.h.min(self.height.saturating_sub(area.y));
        if w > 0 && h > 0 {
            self.blit(&Blit::image(area.x, area.y, RgbImage::new(w, h)));
        }
    }

    /// Write an RGB image at pixel (x, y), clipped to the screen.
    pub fn blit(&self, b: &Blit) {
        if b.x >= self.width || b.y >= self.height {
            return;
        }
        let w = b.w.min(self.width - b.x) as usize;
        let h = b.h.min(self.height - b.y) as usize;
        let bytes_pp = (self.bpp / 8) as usize;
        let mut row = vec![0u8; w * bytes_pp];
        for ry in 0..h {
            let src = &b.rgb[ry * b.w as usize * 3..];
            for cx in 0..w {
                let (r, g, bl) = (src[cx * 3], src[cx * 3 + 1], src[cx * 3 + 2]);
                let dst = &mut row[cx * bytes_pp..];
                if self.bpp == 32 {
                    // XRGB8888 little-endian: B, G, R, X
                    dst[0] = bl;
                    dst[1] = g;
                    dst[2] = r;
                    dst[3] = 0;
                } else {
                    // RGB565
                    let v = ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | (bl as u16 >> 3);
                    dst[0] = (v & 0xFF) as u8;
                    dst[1] = (v >> 8) as u8;
                }
            }
            let off = (b.y as u64 + ry as u64) * self.stride as u64 + b.x as u64 * bytes_pp as u64;
            let _ = self.file.write_at(&row, off);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_overlays_restore_only_the_affected_terminal_rows() {
        let cover = PixelArea {
            x: 80,
            y: 48,
            w: 320,
            h: 320,
        };
        let lyric = PixelArea {
            x: 16,
            y: 384,
            w: 800,
            h: 80,
        };
        let keep = Blit::image(cover.x, cover.y, RgbImage::new(cover.w, cover.h));
        assert_eq!(damaged_areas(&[cover, lyric], &[keep]), vec![lyric]);
        assert_eq!(damaged_rows(&[lyric], 800, 50), vec![24, 25, 26, 27, 28]);
        assert!(damaged_rows(&[lyric], 0, 50).is_empty());
        assert_eq!(damaged_areas(&[cover], &[]), vec![cover]);
    }

    #[test]
    fn pixel_preview_compositing_clips_at_the_surface_edge() {
        let blit = Blit::image(1, 1, RgbImage::from_pixel(4, 4, image::Rgb([20, 40, 60])));
        let mut surface = RgbImage::new(3, 3);
        blit.paint(&mut surface);
        assert_eq!(surface.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(surface.get_pixel(2, 2).0, [20, 40, 60]);
    }

    #[test]
    fn identical_overlays_can_skip_writes_but_moves_and_new_pixels_cannot() {
        let original = Blit::image(8, 16, RgbImage::new(20, 20));
        let mut reused = original.clone();
        assert!(reused.unchanged(std::slice::from_ref(&original)));
        reused.x += 8;
        assert!(!reused.unchanged(std::slice::from_ref(&original)));
        let replacement = Blit::image(8, 16, RgbImage::new(20, 20));
        assert!(!replacement.unchanged(&[original]));
        assert!(!reused.unchanged(&[]));
    }

    #[test]
    fn rgb565_packs() {
        // white -> 0xFFFF, pure red -> 0xF800
        let pack = |r: u8, g: u8, b: u8| {
            ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | (b as u16 >> 3)
        };
        assert_eq!(pack(255, 255, 255), 0xFFFF);
        assert_eq!(pack(255, 0, 0), 0xF800);
        assert_eq!(pack(0, 0, 255), 0x001F);
    }
}
