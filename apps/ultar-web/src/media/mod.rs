//! Sound and video of web pages, played by the browser itself (no browser engine needed):
//! the file is fetched, its sound decoded in pure Rust ([`audio`]: MP3, AAC, FLAC, Vorbis,
//! WAV) and played through the sound server ([`sink`]: a media stream of `ultar-audiod`),
//! its pictures decoded from MP4/H.264 ([`video`]); both follow one clock ([`clock`]).
//! [`player::Player`] runs this on threads and hands the newest picture and its state to the
//! page. Not supported yet: WebM video (VP8/VP9/AV1), Opus sound, streams (HLS/DASH) and
//! sites that build their player with scripts.

pub mod audio;
pub mod clock;
pub mod player;
pub mod relay;
pub mod sink;
pub mod video;

/// Sound of the sound card: 48 kHz, stereo, 16 bit.
pub const RATE: u32 = 48_000;

/// A picture of the video, ready for the canvas (`0xXXRRGGBB` little-endian rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels, `width * height * 4` bytes (blue, green, red, unused).
    pub xrgb: Vec<u8>,
}
