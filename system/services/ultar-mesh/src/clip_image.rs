//! A picture on the clipboard (`image/png`) travels between devices as a series of
//! pieces, because one frame is at most [`crate::wire::MAX_FRAME`] bytes. The receiver
//! takes the pieces strictly in order, never more than [`MAX_IMAGE`] bytes in all, checks
//! the SHA-256 and the PNG signature at the end, and drops what is incomplete after
//! [`ASSEMBLE_TIME`]. The frame carries a version ([`IMAGE_VERSION`]): a piece of a
//! version this device does not know is ignored.

use crate::device;
use crate::wire::CHUNK;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// Largest picture that travels.
pub const MAX_IMAGE: usize = 4 * 1024 * 1024;
/// Version of the piece format.
pub const IMAGE_VERSION: u16 = 1;
/// Longest time the pieces of one picture may take.
pub const ASSEMBLE_TIME: Duration = Duration::from_secs(30);

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// One piece of a picture ([`crate::wire::Frame::ClipImage`]).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Piece {
    /// Version of this format.
    pub v: u16,
    /// Counter of the sender's clipboard.
    pub seq: u64,
    /// Size of the whole picture in bytes.
    pub total: u32,
    /// SHA-256 of the whole picture, hexadecimal.
    pub sha256: String,
    /// Number of this piece, from 0.
    pub index: u32,
    /// The bytes of this piece.
    pub data: String,
}

/// `true` when `bytes` begin like a PNG file.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() > PNG_SIGNATURE.len() && bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE
}

/// SHA-256 of `bytes`, hexadecimal.
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The pieces of `png`; none when it is no PNG or larger than [`MAX_IMAGE`].
pub fn pieces(seq: u64, png: &[u8]) -> Vec<Piece> {
    let Ok(total) = u32::try_from(png.len()) else {
        return Vec::new();
    };
    if !is_png(png) || png.len() > MAX_IMAGE {
        return Vec::new();
    }
    let sha256 = digest(png);
    png.chunks(CHUNK)
        .enumerate()
        .map(|(index, bytes)| Piece {
            v: IMAGE_VERSION,
            seq,
            total,
            sha256: sha256.clone(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
            data: device::encode(bytes),
        })
        .collect()
}

/// A picture that is arriving.
#[derive(Debug)]
pub struct Assembly {
    peer: String,
    seq: u64,
    total: usize,
    sha256: String,
    next: u32,
    bytes: Vec<u8>,
    since: Instant,
}

impl Assembly {
    /// `true` when the pieces took too long.
    pub fn stale(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) > ASSEMBLE_TIME
    }
}

/// What a piece did.
#[derive(Debug, PartialEq, Eq)]
pub enum Taken {
    /// Not acceptable (the picture so far is dropped).
    Refused,
    /// Taken, more to come.
    More,
    /// The picture is complete and correct.
    Done(Vec<u8>),
}

/// Takes `piece` from `peer` into `slot`.
pub fn take(slot: &mut Option<Assembly>, peer: &str, piece: Piece, now: Instant) -> Taken {
    if piece.v != IMAGE_VERSION {
        return Taken::Refused;
    }
    let total = piece.total as usize;
    let Some(data) = device::decode(&piece.data).filter(|d| !d.is_empty() && d.len() <= CHUNK)
    else {
        *slot = None;
        return Taken::Refused;
    };
    if piece.index == 0 {
        if total == 0 || total > MAX_IMAGE || piece.sha256.len() != 64 {
            *slot = None;
            return Taken::Refused;
        }
        *slot = Some(Assembly {
            peer: peer.to_owned(),
            seq: piece.seq,
            total,
            sha256: piece.sha256.clone(),
            next: 0,
            bytes: Vec::with_capacity(total.min(CHUNK * 4)),
            since: now,
        });
    }
    let Some(a) = slot.as_mut() else {
        return Taken::Refused;
    };
    let fits = a.peer == peer
        && a.seq == piece.seq
        && a.total == total
        && a.sha256 == piece.sha256
        && a.next == piece.index
        && a.bytes.len() + data.len() <= a.total
        && !a.stale(now);
    if !fits {
        *slot = None;
        return Taken::Refused;
    }
    a.bytes.extend_from_slice(&data);
    a.next += 1;
    if a.bytes.len() < a.total {
        return Taken::More;
    }
    let Some(done) = slot.take() else {
        return Taken::Refused;
    };
    if digest(&done.bytes) == done.sha256 && is_png(&done.bytes) {
        Taken::Done(done.bytes)
    } else {
        Taken::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(len: usize) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend((0..len.saturating_sub(8)).map(|i| (i % 251) as u8));
        bytes
    }

    fn feed(parts: Vec<Piece>) -> Taken {
        let mut slot = None;
        let now = Instant::now();
        let mut last = Taken::Refused;
        for p in parts {
            last = take(&mut slot, "peer", p, now);
        }
        last
    }

    #[test]
    fn a_picture_survives_its_pieces() {
        let image = png(100_000);
        let parts = pieces(3, &image);
        assert_eq!(parts.len(), 4);
        assert_eq!(feed(parts), Taken::Done(image));
    }

    #[test]
    fn four_mib_travels_and_one_byte_more_does_not() {
        let big = png(MAX_IMAGE);
        assert_eq!(feed(pieces(1, &big)), Taken::Done(big));
        assert!(pieces(1, &png(MAX_IMAGE + 1)).is_empty());
        assert!(pieces(1, b"not a picture at all").is_empty());
    }

    #[test]
    fn a_wrong_hash_a_gap_a_repeat_or_another_sender_is_refused() {
        let image = png(70_000);
        let mut wrong = pieces(1, &image);
        wrong[2].sha256 = digest(b"other");
        assert_eq!(feed(wrong), Taken::Refused);

        let mut gap = pieces(1, &image);
        gap.remove(1);
        assert_eq!(feed(gap), Taken::Refused);

        let mut repeat = pieces(1, &image);
        repeat.insert(2, repeat[1].clone());
        assert_eq!(feed(repeat), Taken::Refused);

        // starting over from the first piece is fine
        let mut again = pieces(1, &image);
        again.insert(1, again[0].clone());
        assert_eq!(feed(again), Taken::Done(image.clone()));

        let mut slot = None;
        let now = Instant::now();
        let parts = pieces(1, &image);
        assert_eq!(take(&mut slot, "a", parts[0].clone(), now), Taken::More);
        assert_eq!(take(&mut slot, "b", parts[1].clone(), now), Taken::Refused);
        assert!(slot.is_none());
    }

    #[test]
    fn a_lie_about_the_size_or_content_is_refused() {
        let image = png(40_000);
        let mut huge = pieces(1, &image);
        huge[0].total = u32::try_from(MAX_IMAGE).unwrap_or(0) + 1;
        assert_eq!(feed(huge), Taken::Refused);

        let mut short = pieces(1, &image);
        short[0].total = 100;
        assert_eq!(feed(short), Taken::Refused);

        // a picture whose hash is right but that is no PNG
        let mut text = b"hello world, this is no picture".to_vec();
        text.resize(1000, b'x');
        let parts: Vec<Piece> = text
            .chunks(CHUNK)
            .enumerate()
            .map(|(i, c)| Piece {
                v: IMAGE_VERSION,
                seq: 1,
                total: 1000,
                sha256: digest(&text),
                index: i as u32,
                data: device::encode(c),
            })
            .collect();
        assert_eq!(feed(parts), Taken::Refused);
    }

    #[test]
    fn a_newer_version_and_a_slow_sender_are_ignored() {
        let image = png(70_000);
        let mut newer = pieces(1, &image);
        newer[0].v = IMAGE_VERSION + 1;
        assert_eq!(feed(newer), Taken::Refused);

        let mut slot = None;
        let now = Instant::now();
        let parts = pieces(1, &image);
        assert_eq!(take(&mut slot, "p", parts[0].clone(), now), Taken::More);
        let late = now + ASSEMBLE_TIME + Duration::from_secs(1);
        assert!(slot.as_ref().is_some_and(|a| a.stale(late)));
        assert_eq!(take(&mut slot, "p", parts[1].clone(), late), Taken::Refused);
    }

    #[test]
    fn a_piece_fits_one_frame() {
        let parts = pieces(u64::MAX, &png(CHUNK * 2));
        let frame = crate::wire::Frame::ClipImage(parts[0].clone());
        assert!(crate::wire::pack(&frame).is_some());
    }
}
