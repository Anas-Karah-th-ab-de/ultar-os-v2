//! Pictures of a document: PNG decoding to the pixel order of the canvas and a small cache.
//! Other picture formats (JPEG, GIF) are kept in the file and saved again unchanged but shown
//! as a frame with their name.

use std::collections::HashMap;
use std::rc::Rc;

/// Most pixels a picture may have (guards memory).
const MAX_PIXELS: u64 = 40_000_000;

/// A decoded picture: BGRX pixels over a white ground.
pub struct Decoded {
    pub w: u32,
    pub h: u32,
    pub xrgb: Vec<u8>,
}

/// `true` for the signature of a JPEG file.
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// Width and height of a JPEG file in pixels (from its frame header).
pub fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if !is_jpeg(b) {
        return None;
    }
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u32::from(u16::from_be_bytes([b[i + 5], b[i + 6]]));
            let w = u32::from(u16::from_be_bytes([b[i + 7], b[i + 8]]));
            return (w > 0 && h > 0).then_some((w, h));
        }
        i += 2 + usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
    }
    None
}

/// Whether `bytes` start like a PNG file.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

/// The size of a PNG in pixels, read from its header (without decoding).
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if !is_png(bytes) || bytes.len() < 24 {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (w, h) = (be(16), be(20));
    (w > 0 && h > 0).then_some((w, h))
}

/// Decodes a PNG into BGRX; transparency is blended over white (the paper).
pub fn decode_png(bytes: &[u8]) -> Option<Decoded> {
    let (w, h) = png_size(bytes)?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..info.buffer_size()];
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return None,
    };
    let mut xrgb = Vec::with_capacity(w as usize * h as usize * 4);
    for px in data.chunks_exact(channels) {
        let (r, g, b, a) = match channels {
            1 => (px[0], px[0], px[0], 255),
            2 => (px[0], px[0], px[0], px[1]),
            3 => (px[0], px[1], px[2], 255),
            _ => (px[0], px[1], px[2], px[3]),
        };
        let over = |c: u8| ((u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a))) / 255) as u8;
        xrgb.extend_from_slice(&[over(b), over(g), over(r), 255]);
    }
    (xrgb.len() == w as usize * h as usize * 4).then_some(Decoded {
        w: info.width,
        h: info.height,
        xrgb,
    })
}

/// Decoded pictures by the identity of their bytes (address and length: the bytes live in
/// the document and do not move while it is open).
#[derive(Default)]
pub struct PictureCache {
    map: HashMap<(usize, usize), Option<Rc<Decoded>>>,
}

impl PictureCache {
    pub fn get(&mut self, bytes: &[u8]) -> Option<Rc<Decoded>> {
        let key = (bytes.as_ptr() as usize, bytes.len());
        if self.map.len() > 64 {
            self.map.clear();
        }
        self.map
            .entry(key)
            .or_insert_with(|| decode_png(bytes).map(Rc::new))
            .clone()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// A 2 x 1 PNG: one opaque red and one fully transparent pixel.
    fn tiny() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().expect("header");
            w.write_image_data(&[255, 0, 0, 255, 0, 0, 0, 0])
                .expect("data");
        }
        out
    }

    #[test]
    fn png_size_and_pixels_over_white() {
        let bytes = tiny();
        assert_eq!(png_size(&bytes), Some((2, 1)));
        let d = decode_png(&bytes).expect("decodes");
        assert_eq!(&d.xrgb[0..4], &[0, 0, 255, 255], "opaque red as BGRX");
        assert_eq!(
            &d.xrgb[4..8],
            &[255, 255, 255, 255],
            "transparent becomes paper"
        );
        assert!(decode_png(b"not a png").is_none());
        assert!(PictureCache::default().get(&bytes).is_some());
    }
}
