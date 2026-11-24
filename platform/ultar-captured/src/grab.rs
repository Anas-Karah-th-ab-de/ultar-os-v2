//! Reading one output's pixels from the compositor.

use std::path::Path;
use std::time::{Duration, Instant};
use ultar_surface::client::Client;
use ultar_surface::proto::{Event, OutputId, Request};

/// Longest time for the whole picture.
pub const DEADLINE: Duration = Duration::from_secs(5);
/// Largest picture accepted (8K).
pub const MAX_SIDE: u32 = 8192;

/// A captured picture: `width * height` pixels `0xXXRRGGBB` little-endian.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Pixels, 4 bytes each.
    pub data: Vec<u8>,
}

/// Why no picture came.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrabError {
    /// No compositor, or no such output.
    NoDisplay,
    /// The compositor sent nothing in time (locked session) or something malformed.
    Failed,
}

impl std::fmt::Display for GrabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDisplay => f.write_str("no display"),
            Self::Failed => f.write_str("capture failed"),
        }
    }
}

impl std::error::Error for GrabError {}

/// Adds one chunk to `frame` (checked: sizes, order, bounds).
pub fn add_chunk(
    frame: &mut Option<Frame>,
    width: u32,
    height: u32,
    row: u32,
    data: &[u8],
) -> Result<(), GrabError> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(GrabError::Failed);
    }
    let f = frame.get_or_insert_with(|| Frame {
        width,
        height,
        data: Vec::new(),
    });
    let stride = width as usize * 4;
    let expected_row = (f.data.len() / stride) as u32;
    if f.width != width
        || f.height != height
        || row != expected_row
        || !data.len().is_multiple_of(stride)
    {
        return Err(GrabError::Failed);
    }
    if f.data.len() + data.len() > stride * height as usize {
        return Err(GrabError::Failed);
    }
    f.data.extend_from_slice(data);
    Ok(())
}

/// Reads `output` (or the primary output) from the compositor at `socket`.
pub fn grab(socket: &Path, output: Option<u32>) -> Result<Frame, GrabError> {
    let mut client = Client::connect(socket, "ultar-captured").map_err(|_| GrabError::NoDisplay)?;
    let start = Instant::now();
    let mut outputs = Vec::new();
    // The outputs arrive right after the connection.
    while let Ok(Some(event)) = client.next_event(Some(Duration::from_millis(300))) {
        if let Event::Output(info) = event {
            outputs.push(info);
        }
        if start.elapsed() > DEADLINE {
            return Err(GrabError::Failed);
        }
    }
    let chosen = match output {
        Some(id) => outputs.iter().find(|o| o.id.0 == id),
        None => outputs.iter().find(|o| o.primary).or(outputs.first()),
    }
    .ok_or(GrabError::NoDisplay)?
    .id;
    client
        .send(&Request::Capture { output: chosen })
        .map_err(|_| GrabError::Failed)?;
    let mut frame = None;
    loop {
        let remaining = DEADLINE.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(GrabError::Failed);
        }
        match client.next_event(Some(remaining)) {
            Ok(Some(Event::CaptureData {
                output: OutputId(o),
                width,
                height,
                row,
                data,
                last,
            })) if o == chosen.0 => {
                add_chunk(&mut frame, width, height, row, &data)?;
                if last {
                    let f = frame.ok_or(GrabError::Failed)?;
                    if f.data.len() != f.width as usize * f.height as usize * 4 {
                        return Err(GrabError::Failed);
                    }
                    return Ok(f);
                }
            }
            Ok(_) => {}
            Err(_) => return Err(GrabError::Failed),
        }
    }
}

/// The picture as a PNG (RGB, 8 bit).
pub fn encode_png(frame: &Frame) -> Result<Vec<u8>, GrabError> {
    let mut rgb = Vec::with_capacity(frame.width as usize * frame.height as usize * 3);
    for px in frame.data.chunks_exact(4) {
        // 0xXXRRGGBB little-endian: B, G, R, X.
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| GrabError::Failed)?;
        writer
            .write_image_data(&rgb)
            .map_err(|_| GrabError::Failed)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_checked() {
        let mut f = None;
        assert_eq!(add_chunk(&mut f, 0, 1, 0, &[]), Err(GrabError::Failed));
        assert_eq!(
            add_chunk(&mut f, MAX_SIDE + 1, 1, 0, &[]),
            Err(GrabError::Failed)
        );
        add_chunk(&mut f, 2, 2, 0, &[1u8; 8]).unwrap();
        assert_eq!(
            add_chunk(&mut f, 2, 2, 0, &[1u8; 8]),
            Err(GrabError::Failed),
            "row again"
        );
        assert_eq!(
            add_chunk(&mut f, 3, 2, 1, &[1u8; 12]),
            Err(GrabError::Failed),
            "other size"
        );
        assert_eq!(
            add_chunk(&mut f, 2, 2, 1, &[1u8; 7]),
            Err(GrabError::Failed),
            "half a row"
        );
        assert_eq!(
            add_chunk(&mut f, 2, 2, 1, &[1u8; 16]),
            Err(GrabError::Failed),
            "too many rows"
        );
        add_chunk(&mut f, 2, 2, 1, &[2u8; 8]).unwrap();
        assert_eq!(f.unwrap().data.len(), 16);
        assert!(!GrabError::NoDisplay.to_string().is_empty());
    }

    #[test]
    fn png_has_the_colours() {
        let frame = Frame {
            width: 1,
            height: 1,
            data: vec![0x30, 0x20, 0x10, 0xff],
        };
        let bytes = encode_png(&frame).unwrap();
        let decoder = png::Decoder::new(bytes.as_slice());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (1, 1));
        assert_eq!(&buf[..3], &[0x10, 0x20, 0x30]);
    }

    #[test]
    fn no_compositor_is_no_display() {
        assert_eq!(
            grab(Path::new("/nonexistent/ultar.sock"), None),
            Err(GrabError::NoDisplay)
        );
    }
}
