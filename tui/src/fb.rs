//! Linux framebuffer blitting. On the text console (fbcon) we can draw real pixels
//! straight into /dev/fb0, so album art renders at full resolution and colour instead
//! of being squeezed through the 16-colour character grid. Geometry comes from sysfs,
//! so no ioctl/libc is needed. Only active when the device is writable (user in the
//! `video` group) and we are on the console (TERM=linux).
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
pub struct Blit {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// RGB triples, row-major, w*h*3 bytes
    pub rgb: Vec<u8>,
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
