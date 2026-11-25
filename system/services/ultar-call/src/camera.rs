//! The camera and the other side's picture of a video call on the device.
//!
//! * [`Camera`]: the pictures of `/dev/ultar/kamera` (Ultar kernel, right `camera`): one MJPEG
//!   frame per read. Cameras leave out the Huffman tables (UVC: the standard ones of JPEG
//!   Annex K.3 are meant); [`with_tables`] puts them in. The picture is scaled to the size a call
//!   sends ([`video::SIZE`]).
//! * [`CallVideo`]: camera plus a sink for the other side's picture ([`PictureSink`]: the phone app
//!   over `call.picture`). Without a camera the call still shows the other side.

use crate::e2e::media::VideoIo;
use crate::e2e::video::{self, Picture};
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// `O_NONBLOCK`.
const NONBLOCK: i32 = 0o4000;
/// Largest picture read from the node.
const MAX_JPEG: usize = 1 << 20;

/// Number of codes per length (1..16) and the values of the four standard tables (Annex K.3):
/// (class and id, bits, values).
const TABLES: [(u8, [u8; 16], &[u8]); 4] = [
    (
        0x00,
        [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        &DC_VALUES,
    ),
    (
        0x01,
        [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
        &DC_VALUES,
    ),
    (
        0x10,
        [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d],
        &AC_LUMA,
    ),
    (
        0x11,
        [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
        &AC_CHROMA,
    ),
];
const DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
#[rustfmt::skip]
const AC_LUMA: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
#[rustfmt::skip]
const AC_CHROMA: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// The standard tables as one DHT segment.
fn dht() -> Vec<u8> {
    let mut body = Vec::new();
    for (tc, bits, values) in TABLES {
        body.push(tc);
        body.extend_from_slice(&bits);
        body.extend_from_slice(values);
    }
    let mut seg = vec![0xFF, 0xC4];
    seg.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    seg.extend_from_slice(&body);
    seg
}

/// The JPEG with the standard Huffman tables put in before the scan when it has none of its own
/// (`None`: not a JPEG).
pub fn with_tables(jpeg: &[u8]) -> Option<Vec<u8>> {
    if jpeg.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut at = 2;
    loop {
        let (&ff, &marker) = (jpeg.get(at)?, jpeg.get(at + 1)?);
        if ff != 0xFF {
            return None;
        }
        match marker {
            0xC4 => return Some(jpeg.to_vec()),
            0xDA => {
                let mut out = Vec::with_capacity(jpeg.len() + 432);
                out.extend_from_slice(&jpeg[..at]);
                out.extend_from_slice(&dht());
                out.extend_from_slice(&jpeg[at..]);
                return Some(out);
            }
            _ => {
                let len = u16::from_be_bytes([*jpeg.get(at + 2)?, *jpeg.get(at + 3)?]);
                at += 2 + usize::from(len);
            }
        }
    }
}

/// `p` scaled to `w`×`h` (nearest pixel).
pub fn fit(p: &Picture, (w, h): (u16, u16)) -> Picture {
    if (p.width, p.height) == (w, h) || p.width == 0 || p.height == 0 {
        return p.clone();
    }
    let (sw, sh) = (usize::from(p.width), usize::from(p.height));
    let mut rgb = Vec::with_capacity(usize::from(w) * usize::from(h) * 3);
    for y in 0..usize::from(h) {
        let sy = y * sh / usize::from(h);
        for x in 0..usize::from(w) {
            let at = (sy * sw + x * sw / usize::from(w)) * 3;
            rgb.extend_from_slice(&p.rgb[at..at + 3]);
        }
    }
    Picture {
        width: w,
        height: h,
        rgb,
    }
}

/// A camera picture as a call sends it.
pub fn from_camera(mjpeg: &[u8]) -> Option<Picture> {
    let p = video::decode(&with_tables(mjpeg)?)?;
    Some(fit(&p, video::SIZE))
}

/// The camera node, read without waiting.
pub struct Camera {
    node: File,
    buf: Vec<u8>,
}

impl Camera {
    /// Opens the node (one program at a time; opening starts the camera).
    pub fn open(path: &Path) -> io::Result<Self> {
        let node = OpenOptions::new()
            .read(true)
            .custom_flags(NONBLOCK)
            .open(path)?;
        Ok(Self {
            node,
            buf: vec![0; MAX_JPEG],
        })
    }

    /// The newest picture (and its JPEG with tables), if one came since the last call.
    pub fn take(&mut self) -> Option<(Picture, Vec<u8>)> {
        let n = self.node.read(&mut self.buf).ok()?;
        let jpeg = with_tables(&self.buf[..n])?;
        let p = video::decode(&jpeg)?;
        Some((fit(&p, video::SIZE), jpeg))
    }
}

/// Where the pictures of a call go to be shown.
pub trait PictureSink: Send {
    /// One picture (JPEG): the own camera's (`own`) or the other side's.
    fn show(&mut self, own: bool, jpeg: Vec<u8>);
}

/// Camera (if any) and the other side's picture of one video call.
pub struct CallVideo {
    /// The camera; `None`: this device sends no picture.
    pub camera: Option<Camera>,
    /// The other side's pictures.
    pub sink: Box<dyn PictureSink>,
}

impl VideoIo for CallVideo {
    fn capture(&mut self) -> Option<Picture> {
        let (p, jpeg) = self.camera.as_mut()?.take()?;
        // the camera's own JPEG for the small own picture: nothing is encoded twice
        self.sink.show(true, jpeg);
        Some(p)
    }
    fn show(&mut self, picture: Picture) {
        if let Some(jpeg) = video::encode(&picture) {
            self.sink.show(false, jpeg);
        }
    }
}

#[cfg(test)]
#[path = "camera_tests.rs"]
mod tests;
