//! Thumbnails of pictures (PNG, JPEG): decoded with reviewed pure-Rust decoders under hard
//! limits (file size, edge length, pixel count, decoder memory), then shrunk with a box
//! filter. Picture files are hostile input; a damaged one gives an error, never a panic.

use super::{Fs, FsError, Loc};
use std::io::Read;

/// Largest picture file that is read.
pub const MAX_FILE: u64 = 16 * 1024 * 1024;
/// Longest edge of a picture that is decoded.
pub const MAX_EDGE: u32 = 8192;
/// Most pixels of a picture that is decoded.
pub const MAX_PIXELS: u64 = 24_000_000;
/// Longest edge of a thumbnail (48 points at scale 2).
pub const THUMB_EDGE: u32 = 96;

/// Why there is no thumbnail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbError {
    /// Not a PNG or JPEG file.
    Unsupported,
    /// Larger than the limits.
    TooLarge,
    /// The decoder refused the data.
    Damaged,
    /// The file could not be read.
    Fs(FsError),
}

impl std::fmt::Display for ThumbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ThumbError::Unsupported => "files.thumb.unsupported",
            ThumbError::TooLarge => "files.thumb.too_large",
            ThumbError::Damaged => "files.thumb.damaged",
            ThumbError::Fs(e) => e.key(),
        })
    }
}

impl std::error::Error for ThumbError {}

/// A small picture, straight-alpha RGBA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels, rows top to bottom.
    pub rgba: Vec<u8>,
}

/// `true` for names the app makes thumbnails of.
pub fn is_picture_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e))
}

/// The thumbnail of the picture file `loc`.
pub fn thumbnail(fs: &Fs, loc: &Loc) -> Result<Picture, ThumbError> {
    let file = fs.open_read(loc).map_err(ThumbError::Fs)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ThumbError::Fs(FsError::from_io(&e)))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(ThumbError::TooLarge);
    }
    from_bytes(&bytes)
}

/// The thumbnail of a picture in memory.
pub fn from_bytes(bytes: &[u8]) -> Result<Picture, ThumbError> {
    let full = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)?
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        decode_jpeg(bytes)?
    } else {
        return Err(ThumbError::Unsupported);
    };
    Ok(shrink(&full, THUMB_EDGE))
}

fn check_size(w: u64, h: u64) -> Result<(), ThumbError> {
    if w == 0 || h == 0 {
        return Err(ThumbError::Damaged);
    }
    if w > u64::from(MAX_EDGE) || h > u64::from(MAX_EDGE) || w * h > MAX_PIXELS {
        return Err(ThumbError::TooLarge);
    }
    Ok(())
}

fn decode_png(bytes: &[u8]) -> Result<Picture, ThumbError> {
    let limits = png::Limits {
        bytes: (MAX_PIXELS * 8) as usize,
    };
    let mut decoder = png::Decoder::new_with_limits(bytes, limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|_| ThumbError::Damaged)?;
    let (w, h) = (reader.info().width, reader.info().height);
    check_size(u64::from(w), u64::from(h))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|_| ThumbError::Damaged)?;
    let data = buf.get(..frame.buffer_size()).ok_or(ThumbError::Damaged)?;
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err(ThumbError::Unsupported),
    };
    picture(frame.width, frame.height, rgba)
}

fn decode_jpeg(bytes: &[u8]) -> Result<Picture, ThumbError> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let options = DecoderOptions::default()
        .set_max_width(MAX_EDGE as usize)
        .set_max_height(MAX_EDGE as usize)
        .jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder =
        zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(bytes), options);
    decoder.decode_headers().map_err(|_| ThumbError::Damaged)?;
    let (w, h) = decoder.dimensions().ok_or(ThumbError::Damaged)?;
    check_size(w as u64, h as u64)?;
    let rgba = decoder.decode().map_err(|_| ThumbError::Damaged)?;
    picture(w as u32, h as u32, rgba)
}

fn picture(width: u32, height: u32, rgba: Vec<u8>) -> Result<Picture, ThumbError> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(ThumbError::Damaged);
    }
    Ok(Picture {
        width,
        height,
        rgba,
    })
}

/// Shrinks `p` so that its longer edge is at most `edge` (box filter; smaller pictures stay).
pub fn shrink(p: &Picture, edge: u32) -> Picture {
    let longer = p.width.max(p.height);
    if longer <= edge {
        return p.clone();
    }
    let w = (u64::from(p.width) * u64::from(edge) / u64::from(longer)).max(1) as u32;
    let h = (u64::from(p.height) * u64::from(edge) / u64::from(longer)).max(1) as u32;
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        let (y0, y1) = span(y, h, p.height);
        for x in 0..w {
            let (x0, x1) = span(x, w, p.width);
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy as usize * p.width as usize + sx as usize) * 4;
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += u64::from(p.rgba[i + k]);
                    }
                }
            }
            let n = u64::from((y1 - y0) * (x1 - x0)).max(1);
            out.extend(sum.iter().map(|s| (s / n) as u8));
        }
    }
    Picture {
        width: w,
        height: h,
        rgba: out,
    }
}

fn span(i: u32, small: u32, big: u32) -> (u32, u32) {
    let a = (u64::from(i) * u64::from(big) / u64::from(small)) as u32;
    let b = (u64::from(i + 1) * u64::from(big) / u64::from(small)) as u32;
    (a, b.max(a + 1).min(big))
}
