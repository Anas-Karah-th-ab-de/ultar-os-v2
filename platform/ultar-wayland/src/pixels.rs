//! Pixels the bridge holds: a copy of each surface's content (so client
//! buffers go back to the app at once and a client that shrinks its memory
//! file can never crash the bridge – rows are read with `pread`, never
//! mapped), composition of subsurfaces, cropping to the window geometry and
//! scaling to the compositor's scale.

use std::os::fd::BorrowedFd;
use ultar_surface::proto::Rect;

/// A picture: `width × height` words `0xAARRGGBB` (alpha premultiplied).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels, row by row.
    pub data: Vec<u32>,
}

impl Image {
    /// A transparent picture.
    pub fn new(width: u32, height: u32) -> Image {
        Image {
            width,
            height,
            data: vec![0; width as usize * height as usize],
        }
    }

    /// The pixels as bytes (little endian, the compositor's layout).
    pub fn bytes(&self) -> Vec<u8> {
        self.data.iter().flat_map(|p| p.to_le_bytes()).collect()
    }

    /// The whole picture as a rectangle.
    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }
}

/// Where a client buffer lies in its pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Offset of the first pixel in the pool.
    pub offset: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row.
    pub stride: u32,
    /// The top byte is alpha (else ignored and the pixel is opaque).
    pub alpha: bool,
}

/// Copies the `rects` of a client buffer into `dst` (same size). Rows the
/// file no longer holds read as transparent black.
pub fn read_rects(fd: BorrowedFd<'_>, layout: &Layout, rects: &[Rect], dst: &mut Image) {
    let mut row = Vec::new();
    for r in rects {
        let Some(r) = clip(r, layout.width, layout.height) else {
            continue;
        };
        row.resize(r.width as usize * 4, 0);
        for y in r.y as u32..r.y as u32 + r.height {
            let at = layout.offset + u64::from(y) * u64::from(layout.stride) + r.x as u64 * 4;
            let got = read_full(fd, &mut row, at);
            row[got..].fill(0);
            let base = (y * dst.width) as usize + r.x as usize;
            for (i, px) in row.chunks_exact(4).enumerate() {
                let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                dst.data[base + i] = if layout.alpha { v } else { v | 0xFF00_0000 };
            }
        }
    }
}

fn read_full(fd: BorrowedFd<'_>, buf: &mut [u8], mut at: u64) -> usize {
    let mut done = 0;
    while done < buf.len() {
        match rustix::io::pread(fd, &mut buf[done..], at) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                done += n;
                at += n as u64;
            }
        }
    }
    done
}

/// Draws `src` over `dst` at (`dx`, `dy`) (premultiplied "over").
pub fn blend(dst: &mut Image, src: &Image, dx: i32, dy: i32) {
    for sy in 0..src.height as i32 {
        let y = dy + sy;
        if y < 0 || y >= dst.height as i32 {
            continue;
        }
        for sx in 0..src.width as i32 {
            let x = dx + sx;
            if x < 0 || x >= dst.width as i32 {
                continue;
            }
            let s = src.data[(sy as u32 * src.width + sx as u32) as usize];
            let d = &mut dst.data[(y as u32 * dst.width + x as u32) as usize];
            *d = over(s, *d);
        }
    }
}

fn over(s: u32, d: u32) -> u32 {
    let a = s >> 24;
    if a == 255 {
        return s;
    }
    if a == 0 {
        return d;
    }
    let inv = 255 - a;
    let mut out = 0;
    for shift in [0, 8, 16, 24] {
        let sc = (s >> shift) & 0xFF;
        let dc = (d >> shift) & 0xFF;
        out |= (sc + (dc * inv + 127) / 255).min(255) << shift;
    }
    out
}

/// The part `r` of `src` (clipped).
pub fn crop(src: &Image, r: Rect) -> Image {
    let Some(r) = clip(&r, src.width, src.height) else {
        return Image::new(1, 1);
    };
    let mut out = Image::new(r.width, r.height);
    for y in 0..r.height {
        let from = ((r.y as u32 + y) * src.width + r.x as u32) as usize;
        let to = (y * r.width) as usize;
        out.data[to..to + r.width as usize]
            .copy_from_slice(&src.data[from..from + r.width as usize]);
    }
    out
}

/// `src` scaled to `width × height` (nearest neighbour).
pub fn scale(src: &Image, width: u32, height: u32) -> Image {
    let (width, height) = (width.max(1), height.max(1));
    if (width, height) == (src.width, src.height) {
        return src.clone();
    }
    let mut out = Image::new(width, height);
    for y in 0..height {
        let sy = (u64::from(y) * u64::from(src.height) / u64::from(height)) as u32;
        for x in 0..width {
            let sx = (u64::from(x) * u64::from(src.width) / u64::from(width)) as u32;
            out.data[(y * width + x) as usize] = src.data[(sy * src.width + sx) as usize];
        }
    }
    out
}

/// `r` inside `width × height`, or nothing.
pub fn clip(r: &Rect, width: u32, height: u32) -> Option<Rect> {
    let x0 = i64::from(r.x).clamp(0, i64::from(width));
    let y0 = i64::from(r.y).clamp(0, i64::from(height));
    let x1 = (i64::from(r.x) + i64::from(r.width)).clamp(0, i64::from(width));
    let y1 = (i64::from(r.y) + i64::from(r.height)).clamp(0, i64::from(height));
    (x1 > x0 && y1 > y0)
        .then(|| Rect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32))
}

/// `r` multiplied by `factor`.
pub fn scale_rect(r: &Rect, factor: i32) -> Rect {
    let f = factor.max(1);
    Rect::new(
        r.x.saturating_mul(f),
        r.y.saturating_mul(f),
        r.width.saturating_mul(f as u32),
        r.height.saturating_mul(f as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsFd;

    #[test]
    fn reads_rows_and_survives_a_short_file() {
        let mut file = tempfile();
        let px: Vec<u8> = (0..4u32 * 3)
            .flat_map(|i| (0x0010_2030 + i).to_le_bytes())
            .collect();
        file.write_all(&px).unwrap();
        let layout = Layout {
            offset: 0,
            width: 4,
            height: 3,
            stride: 16,
            alpha: false,
        };
        let mut img = Image::new(4, 3);
        read_rects(file.as_fd(), &layout, &[Rect::new(0, 0, 4, 3)], &mut img);
        assert_eq!(img.data[0], 0xFF10_2030);
        assert_eq!(img.data[11], 0xFF10_203B);
        let tall = Layout {
            height: 6,
            ..layout
        };
        let mut big = Image::new(4, 6);
        read_rects(
            file.as_fd(),
            &tall,
            &[Rect::new(-5, -5, 100, 100)],
            &mut big,
        );
        assert_eq!(
            big.data[23], 0xFF00_0000,
            "beyond the file: black, no crash"
        );
    }

    fn tempfile() -> std::fs::File {
        let path = std::env::temp_dir().join(format!("uwl-px-{}", std::process::id()));
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let _ = std::fs::remove_file(&path);
        f
    }

    #[test]
    fn blend_crop_scale() {
        let mut dst = Image {
            width: 2,
            height: 2,
            data: vec![0xFF00_00FF; 4],
        };
        let src = Image {
            width: 1,
            height: 1,
            data: vec![0x8080_0000],
        };
        blend(&mut dst, &src, 1, 1);
        assert_eq!(dst.data[3], 0xFF80_007F);
        blend(&mut dst, &src, 5, -3);
        assert_eq!(dst.data[0], 0xFF00_00FF);
        let c = crop(&dst, Rect::new(1, 1, 9, 9));
        assert_eq!((c.width, c.height, c.data[0]), (1, 1, 0xFF80_007F));
        let s = scale(&dst, 4, 4);
        assert_eq!((s.width, s.data[15]), (4, 0xFF80_007F));
        assert_eq!(clip(&Rect::new(i32::MAX, 0, u32::MAX, 1), 8, 8), None);
        assert_eq!(scale_rect(&Rect::new(1, 2, 3, 4), 2), Rect::new(2, 4, 6, 8));
    }
}
