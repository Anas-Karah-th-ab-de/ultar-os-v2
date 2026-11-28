//! A small lossless image codec for remote view (QOI-style), pure Rust and dependency
//! free. One damaged rectangle of a screen is a tiny picture: a QOI image of `width *
//! height` pixels, four bytes each (`0xXXRRGGBB` in memory, but the codec treats the four
//! bytes opaquely and reproduces them exactly).
//!
//! Chosen over a general codec because screen rectangles repeat, are mostly flat and
//! change little between frames: runs, a rolling 64-colour index and delta/luma ops cover
//! exactly those patterns without a dictionary or a window. No new external crate was
//! needed and the format is self-contained (a 14-byte header, then tagged chunks).

/// Bytes per pixel.
pub const BYTES_PER_PIXEL: usize = 4;
/// Most pixels one rectangle may hold (encoder and decoder both refuse more, so a hostile
/// header cannot make the decoder allocate gigabytes).
pub const MAX_PIXELS: usize = 128 * 128;

const OP_INDEX: u8 = 0x00;
const OP_DIFF: u8 = 0x40;
const OP_LUMA: u8 = 0x80;
const OP_RUN: u8 = 0xc0;
const OP_RGB: u8 = 0xfe;
const OP_RGBA: u8 = 0xff;

/// QOI colour hash (0..=63).
fn hash(r: u8, g: u8, b: u8, a: u8) -> u8 {
    let n = (u32::from(r).wrapping_mul(3))
        .wrapping_add(u32::from(g).wrapping_mul(5))
        .wrapping_add(u32::from(b).wrapping_mul(7))
        .wrapping_add(u32::from(a).wrapping_mul(11));
    (n & 63) as u8
}

/// The running state of encoder and decoder: the 64-colour index and the previous pixel.
struct Colours {
    seen: [[u8; 4]; 64],
    prev: [u8; 4],
}

impl Colours {
    fn new() -> Colours {
        Colours {
            seen: [[0; 4]; 64],
            prev: [0, 0, 0, 255],
        }
    }

    fn index_of(px: [u8; 4]) -> u8 {
        hash(px[0], px[1], px[2], px[3])
    }

    /// `px` is the new previous pixel and enters the index.
    fn take(&mut self, px: [u8; 4]) {
        self.seen[Self::index_of(px) as usize] = px;
        self.prev = px;
    }
}

/// Writes one pixel that differs from the previous one (index, small delta, luma or raw).
fn encode_pixel(out: &mut Vec<u8>, colours: &mut Colours, px: [u8; 4]) {
    let prev = colours.prev;
    let index = Colours::index_of(px);
    let dr = i16::from(px[0]) - i16::from(prev[0]);
    let dg = i16::from(px[1]) - i16::from(prev[1]);
    let db = i16::from(px[2]) - i16::from(prev[2]);
    let same_alpha = px[3] == prev[3];
    if colours.seen[index as usize] == px {
        out.push(OP_INDEX | index);
    } else if same_alpha && [dr, dg, db].iter().all(|d| (-2..=1).contains(d)) {
        out.push(OP_DIFF | (((dr + 2) as u8) << 4) | (((dg + 2) as u8) << 2) | ((db + 2) as u8));
    } else if same_alpha
        && (-32..=31).contains(&dg)
        && (-8..=7).contains(&(dr - dg))
        && (-8..=7).contains(&(db - dg))
    {
        out.push(OP_LUMA | ((dg + 32) as u8));
        out.push((((dr - dg + 8) as u8) << 4) | ((db - dg + 8) as u8));
    } else {
        out.push(OP_RGBA);
        out.extend_from_slice(&px);
    }
    colours.take(px);
}

/// Encodes `width * height` pixels (4 bytes each) losslessly as a QOI image; `None` when
/// the size is zero or `pixels` has the wrong length.
pub fn encode(width: u32, height: u32, pixels: &[u8]) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    let count = w.checked_mul(h)?.checked_mul(BYTES_PER_PIXEL)?;
    if w == 0 || h == 0 || w * h > MAX_PIXELS || pixels.len() != count {
        return None;
    }
    let mut out = Vec::with_capacity(count / 2 + 32);
    out.extend_from_slice(b"qoif");
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&[4, 0]); // four channels, sRGB with linear alpha
    let mut colours = Colours::new();
    let mut run = 0u8;
    for p in pixels.chunks_exact(4) {
        let px = [p[0], p[1], p[2], p[3]];
        if px == colours.prev {
            run += 1;
            if run == 62 {
                out.push(OP_RUN | (run - 1));
                run = 0;
            }
            continue;
        }
        if run > 0 {
            out.push(OP_RUN | (run - 1));
            run = 0;
        }
        encode_pixel(&mut out, &mut colours, px);
    }
    if run > 0 {
        out.push(OP_RUN | (run - 1));
    }
    // QOI end marker: seven zero bytes and one 0x01.
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
    Some(out)
}

/// Size and byte count of the image whose header starts `data`; `None` for a bad or too
/// large one.
fn read_header(data: &[u8]) -> Option<(u32, u32, usize)> {
    if data.len() < 22 || &data[..4] != b"qoif" {
        return None;
    }
    let width = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let height = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if data[12] != 4 || width == 0 || height == 0 {
        return None;
    }
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels > MAX_PIXELS {
        return None;
    }
    Some((width, height, pixels.checked_mul(BYTES_PER_PIXEL)?))
}

/// Reads a byte, but never past the end of the data.
fn byte(data: &[u8], at: &mut usize) -> Option<u8> {
    let b = *data.get(*at)?;
    *at += 1;
    Some(b)
}

/// Unsigned `bits`-wide field to its signed value (bias `1 << (bits - 1)`).
fn signed(b: u8, bits: u32) -> i16 {
    i16::from(b & ((1 << bits) - 1)) - (1 << (bits - 1))
}

fn add(b: u8, d: i16) -> u8 {
    (i16::from(b) + d).rem_euclid(256) as u8
}

/// One chunk of the stream: the pixels it stands for go to `out`; `None` for a bad chunk.
fn decode_chunk(data: &[u8], at: &mut usize, colours: &mut Colours, out: &mut Vec<u8>, count: usize) -> Option<()> {
    let op = byte(data, at)?;
    let prev = colours.prev;
    let px = match op {
        OP_RGB => [byte(data, at)?, byte(data, at)?, byte(data, at)?, prev[3]],
        OP_RGBA => [byte(data, at)?, byte(data, at)?, byte(data, at)?, byte(data, at)?],
        _ => match op & 0xc0 {
            OP_INDEX => colours.seen[(op & 0x3f) as usize],
            OP_DIFF => [
                add(prev[0], signed(op >> 4, 2)),
                add(prev[1], signed(op >> 2, 2)),
                add(prev[2], signed(op, 2)),
                prev[3],
            ],
            OP_LUMA => {
                let dg = i16::from(op & 0x3f) - 32;
                let mid = byte(data, at)?;
                [
                    add(prev[0], signed(mid >> 4, 4) + dg),
                    add(prev[1], dg),
                    add(prev[2], signed(mid, 4) + dg),
                    prev[3],
                ]
            }
            _ => {
                for _ in 0..(op & 0x3f) + 1 {
                    if out.len() < count {
                        out.extend_from_slice(&prev);
                    }
                }
                return Some(());
            }
        },
    };
    colours.take(px);
    out.extend_from_slice(&px);
    Some(())
}

/// Decodes a QOI image into `(width, height, pixels)`; `None` when the bytes are no such
/// image (bad magic, bad channels, bad size, truncated or malformed).
pub fn decode(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let (width, height, count) = read_header(data)?;
    let mut out = Vec::with_capacity(count);
    let mut colours = Colours::new();
    let mut at = 14;
    while out.len() < count {
        decode_chunk(data, &mut at, &mut colours, &mut out, count)?;
    }
    // The eight end-marker bytes must follow (seven zero bytes and one 0x01), so a
    // truncated picture is caught.
    let end = data.get(at..at + 8)?;
    if end != [0, 0, 0, 0, 0, 0, 0, 1] {
        return None;
    }
    (out.len() == count).then_some((width, height, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        px.iter()
            .cycle()
            .take(w as usize * h as usize * 4)
            .copied()
            .collect()
    }

    #[test]
    fn a_solid_rectangle_round_trips_and_is_small() {
        let px = solid(64, 48, [0x20, 0x40, 0x60, 0xff]);
        let enc = encode(64, 48, &px).unwrap();
        assert_eq!(decode(&enc), Some((64, 48, px.clone())));
        assert!(
            enc.len() < px.len() / 10,
            "runs should collapse a flat image"
        );
    }

    #[test]
    fn a_gradient_round_trips() {
        let w = 32u32;
        let h = 32u32;
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x as u8) * 3, (y as u8) * 5, (x ^ y) as u8, 0xff]);
            }
        }
        let enc = encode(w, h, &px).unwrap();
        assert_eq!(decode(&enc), Some((w, h, px)));
    }

    #[test]
    fn noise_round_trips_exactly() {
        // A deterministic "noise" that defeats runs, index and diffs.
        let w = 16u32;
        let h = 16u32;
        let mut px = Vec::new();
        for i in 0..(w * h * 4) {
            px.push((i.wrapping_mul(73).wrapping_add(11) & 0xff) as u8);
        }
        let enc = encode(w, h, &px).unwrap();
        assert_eq!(decode(&enc), Some((w, h, px)));
    }

    #[test]
    fn a_run_longer_than_62_splits() {
        let px = solid(100, 1, [9, 9, 9, 9]);
        let enc = encode(100, 1, &px).unwrap();
        assert_eq!(decode(&enc), Some((100, 1, px)));
    }

    #[test]
    fn bad_inputs_are_rejected() {
        assert_eq!(encode(0, 1, &[]), None);
        assert_eq!(encode(1, 1, &[0, 0, 0]), None);
        assert_eq!(decode(b"nope"), None);
        let good = encode(4, 4, &solid(4, 4, [1, 2, 3, 4])).unwrap();
        assert_eq!(decode(&good[..10]), None);
        assert_eq!(decode(&good[..good.len() - 1]), None);
    }

    #[test]
    fn a_hostile_header_cannot_make_the_decoder_allocate() {
        let mut huge = b"qoif".to_vec();
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        huge.extend_from_slice(&[4, 0]);
        huge.extend_from_slice(&[0; 16]);
        assert_eq!(decode(&huge), None);
        assert_eq!(encode(129, 128, &vec![0; 129 * 128 * 4]), None);
    }
}
