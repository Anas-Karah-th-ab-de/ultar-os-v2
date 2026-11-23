//! Surfaces and buffers of one client, and the commit that copies a
//! buffer's damaged pixels into compositor memory.
//!
//! The copy is deliberate: after a commit the compositor owns the pixels,
//! the buffer is released at once, a client can neither change what is on
//! the screen behind the compositor's back nor stall composition, and a
//! client that dies mid-frame leaves a consistent last image.

use crate::geom::Rect;
use crate::pixels::Image;
use ultar_hal_gfx::shm::SealedMapping;
use ultar_surface::proto::{PixelFormat, Rect as WireRect, Role, SurfaceId};

/// Pixels a buffer holds.
#[derive(Debug)]
pub enum Storage {
    /// Shared memory of the client.
    Shared {
        /// The mapping.
        mapping: SealedMapping,
        /// Bytes per row.
        stride: u32,
    },
    /// Pixels sent inline, kept here.
    Inline(Image),
}

/// A client buffer.
#[derive(Debug)]
pub struct Buffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel layout.
    pub format: PixelFormat,
    /// Where the pixels are.
    pub storage: Storage,
}

impl Buffer {
    /// Memory charged to the client for this buffer.
    pub fn charge(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }

    /// Writes inline pixels; false if the buffer is shared or the data
    /// does not match the rectangle.
    pub fn write(&mut self, rect: &WireRect, data: &[u8]) -> bool {
        let Storage::Inline(image) = &mut self.storage else {
            return false;
        };
        let r = Rect::new(rect.x, rect.y, rect.width as i32, rect.height as i32);
        if rect.width > self.width
            || rect.height > self.height
            || !image.bounds().contains_rect(&r)
            || r.is_empty()
        {
            return false;
        }
        if data.len() as u64 != u64::from(rect.width) * u64::from(rect.height) * 4 {
            return false;
        }
        let width = image.width() as usize;
        let pixels = image.data_mut();
        for (row, chunk) in data.chunks_exact(rect.width as usize * 4).enumerate() {
            let start = (rect.y as usize + row) * width + rect.x as usize;
            for (px, b) in pixels[start..start + rect.width as usize]
                .iter_mut()
                .zip(chunk.chunks_exact(4))
            {
                *px = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
        }
        true
    }

    /// Copies `rect` (clipped) into `dst` (same size as the buffer).
    /// Opaque formats get alpha 255.
    pub fn copy_into(&self, dst: &mut Image, rect: Rect) -> bool {
        let Some(r) = rect.intersect(&dst.bounds()) else {
            return true;
        };
        let force = if self.format == PixelFormat::Xrgb8888 {
            0xff00_0000
        } else {
            0
        };
        let width = dst.width() as usize;
        let pixels = dst.data_mut();
        for y in r.y..r.max_y() {
            let start = y as usize * width + r.x as usize;
            let row = &mut pixels[start..start + r.w as usize];
            match &self.storage {
                Storage::Shared { mapping, stride } => {
                    let offset = y as usize * *stride as usize + r.x as usize * 4;
                    if mapping.copy_pixels(offset, row).is_err() {
                        return false;
                    }
                }
                Storage::Inline(image) => {
                    let s = y as usize * image.width() as usize + r.x as usize;
                    row.copy_from_slice(&image.data()[s..s + r.w as usize]);
                }
            }
            if force != 0 {
                row.iter_mut().for_each(|p| *p |= force);
            }
        }
        true
    }
}

/// Size limits a client asked for (logical points, 0 = none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SizeLimits {
    /// Smallest content size.
    pub min: (u32, u32),
    /// Largest content size.
    pub max: (u32, u32),
}

/// A surface of a client.
#[derive(Debug)]
pub struct Surface {
    /// Id in the client's namespace.
    pub id: SurfaceId,
    /// Role.
    pub role: Role,
    /// Title (toplevels).
    pub title: String,
    /// Committed pixels.
    pub image: Option<Image>,
    /// Committed content has no transparency.
    pub opaque: bool,
    /// Scale the content was drawn for (× 1000).
    pub scale_milli: u32,
    /// Buffer chosen for the next commit (`Some(0)` = detach).
    pub pending_buffer: Option<u32>,
    /// Damage for the next commit (buffer pixels).
    pub pending_damage: Vec<Rect>,
    /// A frame callback is wanted.
    pub frame_requested: bool,
    /// Size limits.
    pub limits: SizeLimits,
    /// Serial of the last configure sent.
    pub configure_sent: u32,
    /// Serial of the last configure the client acknowledged.
    pub configure_acked: u32,
    /// Scale of each configure serial sent and not yet acknowledged.
    pub configure_scales: Vec<(u32, u32)>,
    /// A text field of this surface wants the on-screen keyboard (purpose).
    pub text_input: Option<u8>,
    /// The last committed buffer was shared memory (not inline pixels).
    pub shared: bool,
}

impl Surface {
    /// A new surface without content.
    pub fn new(id: SurfaceId, role: Role) -> Surface {
        Surface {
            id,
            role,
            title: String::new(),
            image: None,
            opaque: true,
            scale_milli: 1000,
            pending_buffer: None,
            pending_damage: Vec::new(),
            frame_requested: false,
            limits: SizeLimits::default(),
            configure_sent: 0,
            configure_acked: 0,
            configure_scales: Vec::new(),
            text_input: None,
            shared: false,
        }
    }

    /// Memory charged for the committed image.
    pub fn charge(&self) -> u64 {
        self.image
            .as_ref()
            .map_or(0, |i| u64::from(i.width()) * u64::from(i.height()) * 4)
    }

    /// Logical size of the content (pixels / scale).
    pub fn logical_size(&self) -> Option<(i32, i32)> {
        let image = self.image.as_ref()?;
        let s = f64::from(self.scale_milli) / 1000.0;
        Some((
            (f64::from(image.width()) / s).round() as i32,
            (f64::from(image.height()) / s).round() as i32,
        ))
    }

    /// Records the acknowledgement of a configure and adopts its scale.
    pub fn ack(&mut self, serial: u32) {
        if serial == 0 || serial > self.configure_sent {
            return;
        }
        if let Some(&(_, scale)) = self
            .configure_scales
            .iter()
            .rev()
            .find(|(s, _)| *s <= serial)
        {
            self.scale_milli = scale;
        }
        self.configure_scales.retain(|(s, _)| *s > serial);
        self.configure_acked = self.configure_acked.max(serial);
    }

    /// Applies a commit with `buffer` (None = keep content): copies the
    /// damage into the surface image. Returns the damaged rectangles in
    /// surface pixels, or `None` if the buffer cannot be read.
    pub fn apply(&mut self, buffer: Option<&Buffer>) -> Option<Vec<Rect>> {
        let damage = std::mem::take(&mut self.pending_damage);
        let Some(buffer) = buffer else {
            return Some(Vec::new());
        };
        let fresh = !matches!(&self.image, Some(i) if i.width() == buffer.width && i.height() == buffer.height);
        if fresh {
            self.image = Some(Image::new(buffer.width, buffer.height, 0)?);
        }
        let image = self.image.as_mut()?;
        let rects = if fresh || damage.is_empty() {
            vec![image.bounds()]
        } else {
            damage
        };
        for r in &rects {
            if !buffer.copy_into(image, *r) {
                return None;
            }
        }
        self.opaque = buffer.format == PixelFormat::Xrgb8888;
        Some(
            rects
                .iter()
                .filter_map(|r| r.intersect(&image.bounds()))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_hal_gfx::shm::SharedBuffer;

    fn inline(w: u32, h: u32, format: PixelFormat) -> Buffer {
        Buffer {
            width: w,
            height: h,
            format,
            storage: Storage::Inline(Image::new(w, h, 0).unwrap()),
        }
    }

    #[test]
    fn inline_writes_are_checked() {
        let mut b = inline(4, 4, PixelFormat::Xrgb8888);
        assert_eq!(b.charge(), 64);
        assert!(b.write(&WireRect::new(1, 1, 2, 1), &[1, 0, 0, 0, 2, 0, 0, 0]));
        assert!(!b.write(&WireRect::new(1, 1, 2, 1), &[1, 0, 0, 0]));
        assert!(!b.write(&WireRect::new(3, 3, 2, 2), &[0; 16]));
        assert!(!b.write(&WireRect::new(-1, 0, 1, 1), &[0; 4]));
        assert!(!b.write(&WireRect::new(0, 0, 0, 0), &[]));
        assert!(!b.write(&WireRect::new(0, 0, u32::MAX, 1), &[]));
        let mut s = Surface::new(SurfaceId(1), Role::Toplevel);
        let rects = s.apply(Some(&b)).unwrap();
        assert_eq!(rects, vec![Rect::new(0, 0, 4, 4)]);
        assert_eq!(s.image.as_ref().unwrap().pixel(2, 1), Some(0xff00_0002));
        assert_eq!(s.charge(), 64);
        assert!(s.opaque);
        // Second commit with damage copies only the damage.
        b.write(&WireRect::new(0, 0, 1, 1), &[9, 0, 0, 0]);
        b.write(&WireRect::new(3, 3, 1, 1), &[9, 0, 0, 0]);
        s.pending_damage = vec![Rect::new(0, 0, 1, 1)];
        assert_eq!(s.apply(Some(&b)).unwrap(), vec![Rect::new(0, 0, 1, 1)]);
        assert_eq!(s.image.as_ref().unwrap().pixel(0, 0), Some(0xff00_0009));
        assert_eq!(s.image.as_ref().unwrap().pixel(3, 3), Some(0xff00_0000));
        assert_eq!(s.apply(None).unwrap(), Vec::<Rect>::new());
        assert_eq!(s.logical_size(), Some((4, 4)));
    }

    #[test]
    fn shared_buffers_and_acks() {
        let mut shared = SharedBuffer::create(2 * 2 * 4).unwrap();
        shared.bytes_mut()[0..4].copy_from_slice(&0x8000_0080u32.to_le_bytes());
        let mapping = SealedMapping::map(shared.fd().try_clone_to_owned().unwrap(), 16).unwrap();
        let b = Buffer {
            width: 2,
            height: 2,
            format: PixelFormat::Argb8888,
            storage: Storage::Shared { mapping, stride: 8 },
        };
        let mut s = Surface::new(SurfaceId(2), Role::Toplevel);
        s.apply(Some(&b)).unwrap();
        assert!(!s.opaque);
        assert_eq!(s.image.as_ref().unwrap().pixel(0, 0), Some(0x8000_0080));
        let mut w = inline(2, 2, PixelFormat::Xrgb8888);
        assert!(!Buffer {
            storage: Storage::Shared {
                mapping: SealedMapping::map(shared.fd().try_clone_to_owned().unwrap(), 16).unwrap(),
                stride: 8
            },
            ..inline(2, 2, PixelFormat::Xrgb8888)
        }
        .write(&WireRect::new(0, 0, 1, 1), &[0; 4]));
        // A stride that reads past the mapping fails the commit.
        let bad = Buffer {
            width: 2,
            height: 2,
            format: PixelFormat::Xrgb8888,
            storage: Storage::Shared {
                mapping: SealedMapping::map(shared.fd().try_clone_to_owned().unwrap(), 16).unwrap(),
                stride: 4096,
            },
        };
        let mut s2 = Surface::new(SurfaceId(3), Role::Toplevel);
        assert!(s2.apply(Some(&bad)).is_none());
        assert!(w.write(&WireRect::new(0, 0, 1, 1), &[0; 4]));
        s.configure_sent = 3;
        s.configure_scales = vec![(2, 2000), (3, 1500)];
        s.ack(9);
        assert_eq!(s.configure_acked, 0);
        s.ack(2);
        assert_eq!((s.configure_acked, s.scale_milli), (2, 2000));
        s.ack(3);
        assert_eq!((s.configure_acked, s.scale_milli), (3, 1500));
        assert_eq!(s.logical_size(), Some((1, 1)));
    }
}
