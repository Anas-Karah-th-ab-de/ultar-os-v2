//! Picture of a video call: each frame as a JPEG (pure Rust encoder and decoder), split into
//! pieces that fit a datagram and put together again on the other side. A frame with a missing
//! piece is dropped – the next one follows a moment later.

use std::collections::BTreeMap;

/// Bytes of picture per datagram.
pub const PIECE: usize = 1100;
/// Width and height a call sends.
pub const SIZE: (u16, u16) = (320, 240);
/// JPEG quality (1–100).
const QUALITY: u8 = 55;
/// Frames being put together at once.
const OPEN_FRAMES: usize = 3;
/// Largest frame accepted (bytes), against a peer that sends garbage.
const MAX_FRAME: usize = 255 * PIECE;

/// A picture: RGB, 3 bytes per pixel, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
    /// Pixels.
    pub rgb: Vec<u8>,
}

/// JPEG of a picture; `None` when the size does not match the pixels.
pub fn encode(p: &Picture) -> Option<Vec<u8>> {
    if p.rgb.len() != usize::from(p.width) * usize::from(p.height) * 3 {
        return None;
    }
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, QUALITY);
    enc.encode(&p.rgb, p.width, p.height, jpeg_encoder::ColorType::Rgb)
        .ok()?;
    Some(out)
}

/// The picture in a JPEG.
pub fn decode(jpeg: &[u8]) -> Option<Picture> {
    let mut d = zune_jpeg::JpegDecoder::new(zune_jpeg::zune_core::bytestream::ZCursor::new(jpeg));
    let rgb = d.decode().ok()?;
    let (w, h) = d.dimensions()?;
    Some(Picture {
        width: u16::try_from(w).ok()?,
        height: u16::try_from(h).ok()?,
        rgb,
    })
}

/// Splits frame `id` into datagram pieces: `id (4) ‖ index (1) ‖ count (1) ‖ bytes`.
pub fn pieces(id: u32, jpeg: &[u8]) -> Vec<Vec<u8>> {
    let chunks: Vec<&[u8]> = jpeg.chunks(PIECE).collect();
    let count = chunks.len().min(255) as u8;
    chunks
        .iter()
        .take(255)
        .enumerate()
        .map(|(i, c)| {
            let mut v = id.to_be_bytes().to_vec();
            v.push(i as u8);
            v.push(count);
            v.extend_from_slice(c);
            v
        })
        .collect()
}

/// Puts pieces together into whole frames.
#[derive(Default)]
pub struct Assembler {
    open: BTreeMap<u32, (u8, BTreeMap<u8, Vec<u8>>)>,
    done: u32,
}

impl Assembler {
    /// One piece arrived; returns a frame when it is complete.
    pub fn put(&mut self, piece: &[u8]) -> Option<Vec<u8>> {
        let (head, bytes) = piece.split_at_checked(6)?;
        let id = u32::from_be_bytes(head[..4].try_into().ok()?);
        let (index, count) = (head[4], head[5]);
        if count == 0 || index >= count || (self.done != 0 && id <= self.done) {
            return None;
        }
        let entry = self
            .open
            .entry(id)
            .or_insert_with(|| (count, BTreeMap::new()));
        if entry.0 != count {
            return None;
        }
        entry.1.insert(index, bytes.to_vec());
        while self.open.len() > OPEN_FRAMES {
            self.open.pop_first();
        }
        let complete = self
            .open
            .get(&id)
            .is_some_and(|(n, parts)| parts.len() == usize::from(*n));
        if !complete {
            return None;
        }
        let (_, parts) = self.open.remove(&id)?;
        self.done = id;
        self.open.retain(|k, _| *k > id);
        let frame: Vec<u8> = parts.into_values().flatten().collect();
        (frame.len() <= MAX_FRAME).then_some(frame)
    }
}

/// A test picture: colour bars with a moving band (for a device without a camera to show that
/// the picture arrives, and for tests).
pub fn bars(tick: u32) -> Picture {
    let (w, h) = SIZE;
    let mut rgb = Vec::with_capacity(usize::from(w) * usize::from(h) * 3);
    let colours = [
        [235, 235, 235],
        [235, 235, 16],
        [16, 235, 235],
        [16, 235, 16],
        [235, 16, 235],
        [235, 16, 16],
        [16, 16, 235],
        [16, 16, 16],
    ];
    let band = (tick as usize * 4) % usize::from(h);
    for y in 0..usize::from(h) {
        for x in 0..usize::from(w) {
            let c = if y.abs_diff(band) < 6 {
                [255, 255, 255]
            } else {
                colours[x * 8 / usize::from(w)]
            };
            rgb.extend_from_slice(&c);
        }
    }
    Picture {
        width: w,
        height: h,
        rgb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_goes_through_in_pieces_in_any_order() {
        let p = bars(3);
        let jpeg = encode(&p).unwrap();
        assert!(jpeg.len() < 40_000, "{} bytes", jpeg.len());
        let mut parts = pieces(7, &jpeg);
        assert!(parts.len() > 1);
        parts.reverse();
        let mut a = Assembler::default();
        let mut got = None;
        for part in &parts {
            got = a.put(part).or(got);
        }
        let back = decode(&got.unwrap()).unwrap();
        assert_eq!((back.width, back.height), SIZE);
        // JPEG is lossy: the bars are still the bars
        let px = |p: &Picture, x: usize, y: usize| p.rgb[(y * 320 + x) * 3];
        assert!(px(&back, 10, 120) > 200 && px(&back, 310, 120) < 50);
        assert_eq!(a.put(&parts[0]), None, "an old frame is not taken again");
    }

    #[test]
    fn incomplete_or_odd_pieces_give_nothing() {
        let jpeg = encode(&bars(0)).unwrap();
        let parts = pieces(1, &jpeg);
        let mut a = Assembler::default();
        for p in &parts[1..] {
            assert_eq!(a.put(p), None);
        }
        assert_eq!(a.put(&[0, 0, 0, 2, 5, 3]), None, "index beyond count");
        assert_eq!(a.put(&[1]), None);
        assert_eq!(
            encode(&Picture {
                width: 2,
                height: 2,
                rgb: vec![0; 3]
            }),
            None
        );
        assert_eq!(decode(b"not a jpeg"), None);
    }
}
