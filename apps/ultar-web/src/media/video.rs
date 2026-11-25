//! The pictures of an MP4 file with H.264 video: samples read with the `mp4` crate (pure
//! Rust), turned into an Annex B stream (start codes, SPS/PPS in front) and decoded by
//! OpenH264; each picture as `0xXXRRGGBB` rows, larger ones reduced to [`MAX_WIDTH`].

use super::audio::Bytes;
use super::Picture;
use mp4::{MediaType, Mp4Reader, TrackType};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use std::io::Cursor;
use std::time::Duration;

/// Widest picture kept (a wider video is reduced; the page shows it smaller anyway).
pub const MAX_WIDTH: usize = 1280;
/// The Annex B start code.
const START: [u8; 4] = [0, 0, 0, 1];

/// Why the pictures cannot be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoError {
    /// Not an MP4 file, or no video track.
    NoVideo,
    /// A video codec this player cannot decode (VP9, H.265 …), named.
    Codec(String),
    /// The decoder could not start.
    Decoder,
}

/// The pictures of a video track.
pub struct VideoStream {
    reader: Mp4Reader<Cursor<Bytes>>,
    track: u32,
    next: u32,
    count: u32,
    timescale: u32,
    length: usize,
    header: Vec<u8>,
    decoder: Decoder,
    /// Width and height of the video.
    pub size: (u32, u32),
}

impl VideoStream {
    /// The H.264 track of the MP4 file in `bytes`.
    pub fn open(bytes: Bytes) -> Result<VideoStream, VideoError> {
        let len = bytes.0.len() as u64;
        let reader =
            Mp4Reader::read_header(Cursor::new(bytes), len).map_err(|_| VideoError::NoVideo)?;
        let track = reader
            .tracks()
            .values()
            .find(|t| matches!(t.track_type(), Ok(TrackType::Video)))
            .ok_or(VideoError::NoVideo)?;
        match track.media_type() {
            Ok(MediaType::H264) => {}
            Ok(other) => return Err(VideoError::Codec(other.to_string())),
            Err(_) => return Err(VideoError::Codec("?".into())),
        }
        let mut header = Vec::new();
        for set in [
            track.sequence_parameter_set(),
            track.picture_parameter_set(),
        ] {
            let set = set.map_err(|_| VideoError::Decoder)?;
            header.extend_from_slice(&START);
            header.extend_from_slice(set);
        }
        let length = track
            .trak
            .mdia
            .minf
            .stbl
            .stsd
            .avc1
            .as_ref()
            .map_or(4, |a| usize::from(a.avcc.length_size_minus_one & 3) + 1);
        let (id, count, timescale) = (
            track.track_id(),
            track.sample_count(),
            track.timescale().max(1),
        );
        let size = (u32::from(track.width()), u32::from(track.height()));
        let decoder = Decoder::new().map_err(|_| VideoError::Decoder)?;
        Ok(VideoStream {
            reader,
            track: id,
            next: 1,
            count,
            timescale,
            length,
            header,
            decoder,
            size,
        })
    }

    /// The next picture and when it is shown; `None` at the end. Samples the decoder cannot
    /// use yet (it waits for more) are read on.
    pub fn next_picture(&mut self) -> Option<(Duration, Picture)> {
        while self.next <= self.count {
            let id = self.next;
            self.next += 1;
            let Ok(Some(sample)) = self.reader.read_sample(self.track, id) else {
                continue;
            };
            let shown = sample.start_time as i64 + i64::from(sample.rendering_offset);
            let at = Duration::from_secs_f64(shown.max(0) as f64 / f64::from(self.timescale));
            let mut annex_b = if id == 1 {
                self.header.clone()
            } else {
                Vec::new()
            };
            annex_b.extend(to_annex_b(&sample.bytes, self.length));
            if let Ok(Some(yuv)) = self.decoder.decode(&annex_b) {
                return Some((at, picture(&yuv)));
            }
        }
        None
    }
}

/// Length-prefixed NAL units (MP4) as an Annex B stream.
fn to_annex_b(sample: &[u8], length: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(sample.len() + 16);
    let mut rest = sample;
    while rest.len() > length {
        let n = rest[..length]
            .iter()
            .fold(0usize, |a, b| (a << 8) | usize::from(*b));
        rest = &rest[length..];
        let take = n.min(rest.len());
        out.extend_from_slice(&START);
        out.extend_from_slice(&rest[..take]);
        rest = &rest[take..];
    }
    out
}

/// A decoded picture as `0xXXRRGGBB` rows, at most [`MAX_WIDTH`] wide.
fn picture(yuv: &impl YUVSource) -> Picture {
    let (w, h) = yuv.dimensions();
    let mut rgb = vec![0u8; w * h * 3];
    yuv_to_rgb(yuv, &mut rgb, w, h);
    let step = w.div_ceil(MAX_WIDTH).max(1);
    let (ow, oh) = (w / step, h / step);
    let mut xrgb = Vec::with_capacity(ow * oh * 4);
    for y in 0..oh {
        for x in 0..ow {
            let at = ((y * step) * w + x * step) * 3;
            xrgb.extend_from_slice(&[rgb[at + 2], rgb[at + 1], rgb[at], 0xFF]);
        }
    }
    Picture {
        width: ow as u32,
        height: oh as u32,
        xrgb,
    }
}

/// BT.601 limited range YUV 4:2:0 → RGB.
fn yuv_to_rgb(yuv: &impl YUVSource, rgb: &mut [u8], w: usize, h: usize) {
    let (sy, su, sv) = yuv.strides();
    let (py, pu, pv) = (yuv.y(), yuv.u(), yuv.v());
    for row in 0..h {
        for col in 0..w {
            let y = f32::from(py[row * sy + col]) - 16.0;
            let u = f32::from(pu[(row / 2) * su + col / 2]) - 128.0;
            let v = f32::from(pv[(row / 2) * sv + col / 2]) - 128.0;
            let c = 1.164 * y;
            let at = (row * w + col) * 3;
            rgb[at] = (c + 1.596 * v).clamp(0.0, 255.0) as u8;
            rgb[at + 1] = (c - 0.392 * u - 0.813 * v).clamp(0.0, 255.0) as u8;
            rgb[at + 2] = (c + 2.017 * u).clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
#[path = "video_tests.rs"]
pub(crate) mod tests;
