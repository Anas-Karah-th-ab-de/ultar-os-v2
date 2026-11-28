//! The frame buffer of a tab: sealed-size shared memory (`memfd`) that the browser reads by
//! rows. Servo paints RGBA; the browser shows BGRX, so every row is converted here. Only rows
//! that differ from the last frame are written and reported as damage.

use rustix::fs::{memfd_create, MemfdFlags};
use std::fs::File;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::FileExt;
use ultar_web::engine::FrameDamage;

/// The shared memory of one tab at one size.
pub struct FrameBuffer {
    file: File,
    width: u32,
    height: u32,
    last: Vec<u8>,
}

impl FrameBuffer {
    /// A buffer of `width × height` pixels (4 bytes each, rows without padding).
    pub fn new(width: u32, height: u32) -> io::Result<FrameBuffer> {
        let fd = memfd_create("ultar-web-frame", MemfdFlags::CLOEXEC)?;
        let file = File::from(fd);
        let size = u64::from(width) * u64::from(height) * 4;
        file.set_len(size)?;
        Ok(FrameBuffer {
            file,
            width,
            height,
            last: vec![0; size as usize],
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Bytes per row.
    pub fn stride(&self) -> u32 {
        self.width * 4
    }

    /// The descriptor to pass to the browser.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }

    /// Takes a new RGBA picture of the same size (rows of `width × 4` bytes) and writes the
    /// rows that changed. Gives the changed band, `None` when nothing changed.
    pub fn update(&mut self, rgba: &[u8]) -> io::Result<Option<FrameDamage>> {
        let stride = self.stride() as usize;
        if rgba.len() < stride * self.height as usize {
            return Err(io::Error::other("picture smaller than the buffer"));
        }
        let mut row = vec![0u8; stride];
        let (mut first, mut last) = (None, 0);
        for y in 0..self.height as usize {
            to_bgrx(&rgba[y * stride..(y + 1) * stride], &mut row);
            let old = &mut self.last[y * stride..(y + 1) * stride];
            if *old == row[..] {
                continue;
            }
            old.copy_from_slice(&row);
            self.file.write_all_at(&row, (y * stride) as u64)?;
            first.get_or_insert(y);
            last = y;
        }
        Ok(first.map(|y0| FrameDamage {
            x0: 0,
            y0: y0 as u32,
            x1: self.width,
            y1: last as u32 + 1,
        }))
    }
}

/// One row RGBA → BGRX (the alpha byte becomes 255: pages are opaque in the window).
fn to_bgrx(rgba: &[u8], out: &mut [u8]) {
    for (src, dst) in rgba.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn only_changed_rows_are_written_and_reported() {
        let mut fb = FrameBuffer::new(2, 3).unwrap();
        let mut pic = vec![0u8; 2 * 3 * 4];
        let d = fb.update(&pic).unwrap().unwrap();
        assert_eq!((d.y0, d.y1, d.x1), (0, 3, 2), "the first picture fills the new buffer");
        pic[8..12].copy_from_slice(&[10, 20, 30, 0]); // row 1, pixel 0
        let d = fb.update(&pic).unwrap().unwrap();
        assert_eq!((d.y0, d.y1, d.x1), (1, 2, 2));
        assert_eq!(fb.update(&pic).unwrap(), None, "same picture again");
        let mut back = Vec::new();
        File::from(fb.fd().try_clone_to_owned().unwrap())
            .read_to_end(&mut back)
            .unwrap();
        assert_eq!(&back[8..12], &[30, 20, 10, 255]);
        assert_eq!(back.len(), 24);
        assert!(fb.update(&pic[..8]).is_err());
    }
}
