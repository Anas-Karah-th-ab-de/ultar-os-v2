//! Lines a service writes to its standard output or error, as `ultar-init`
//! forwards them to the log service.
//!
//! Records of the logging API carry no free text (see [`super::record`]).
//! What a program prints is free text, so it travels on a channel of its
//! own: only `ultar-init` writes to it (one socket, reachable by init
//! alone), the log service stores the text cleaned of control and
//! direction characters and shows it to readers like the kernel's lines.
//!
//! A frame is the length-prefixed frame of [`super::wire`] (at most
//! [`super::wire::MAX_BODY`] bytes) with this body:
//!
//! ```text
//! stream  u8      1 = standard output, 2 = standard error
//! len     u8      length of the service name
//! name    len     1–32 bytes, a-z 0-9 '-' '_'
//! text    rest    UTF-8, at most MAX_TEXT bytes, no line feed
//! ```

use std::fmt;

/// Why a frame of this channel was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputError {
    /// The body ends early.
    Truncated,
    /// Unknown stream number.
    Stream,
    /// The service name breaks its rules.
    Name,
    /// The text is not UTF-8, too long or contains a line feed.
    Text,
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Truncated => "output line truncated",
            Self::Stream => "unknown output stream",
            Self::Name => "invalid service name in output line",
            Self::Text => "invalid output text",
        })
    }
}

impl std::error::Error for OutputError {}

/// Longest text of one line in bytes; longer lines are cut by the sender.
pub const MAX_TEXT: usize = 512;
/// Longest service name.
pub const MAX_NAME: usize = 32;

/// Which stream a line came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    /// Standard output.
    Out,
    /// Standard error.
    Err,
}

impl Stream {
    /// Wire number.
    pub const fn number(self) -> u8 {
        match self {
            Self::Out => 1,
            Self::Err => 2,
        }
    }

    /// The stream with this wire number.
    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::Out),
            2 => Some(Self::Err),
            _ => None,
        }
    }

    /// Message key the log service stores the line under.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Out => "output.stdout",
            Self::Err => "output.stderr",
        }
    }
}

/// One line of output of a service.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutputLine {
    /// The service (its name in the service files).
    pub service: String,
    /// Where the line came from.
    pub stream: Stream,
    /// The text, without line feed.
    pub text: String,
}

/// Whether `name` is a service name this channel carries.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Cuts `text` to at most [`MAX_TEXT`] bytes at a character boundary.
pub fn cut(text: &str) -> &str {
    if text.len() <= MAX_TEXT {
        return text;
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// Encodes `line` as one frame; the text is cut to [`MAX_TEXT`] bytes.
pub fn encode_frame(line: &OutputLine) -> Result<Vec<u8>, OutputError> {
    if !valid_name(&line.service) {
        return Err(OutputError::Name);
    }
    if line.text.contains('\n') {
        return Err(OutputError::Text);
    }
    let text = cut(&line.text);
    let body_len = 2 + line.service.len() + text.len();
    let mut frame = Vec::with_capacity(body_len + 2);
    let len = u16::try_from(body_len).map_err(|_| OutputError::Text)?;
    frame.extend_from_slice(&len.to_le_bytes());
    frame.push(line.stream.number());
    frame.push(line.service.len() as u8);
    frame.extend_from_slice(line.service.as_bytes());
    frame.extend_from_slice(text.as_bytes());
    Ok(frame)
}

/// Decodes one frame body; everything is checked.
pub fn decode_body(body: &[u8]) -> Result<OutputLine, OutputError> {
    let (&stream, rest) = body.split_first().ok_or(OutputError::Truncated)?;
    let stream = Stream::from_number(stream).ok_or(OutputError::Stream)?;
    let (&len, rest) = rest.split_first().ok_or(OutputError::Truncated)?;
    let len = usize::from(len);
    let name = rest.get(..len).ok_or(OutputError::Truncated)?;
    let text = rest.get(len..).ok_or(OutputError::Truncated)?;
    let service = std::str::from_utf8(name).map_err(|_| OutputError::Name)?;
    if !valid_name(service) {
        return Err(OutputError::Name);
    }
    let text = std::str::from_utf8(text).map_err(|_| OutputError::Text)?;
    if text.len() > MAX_TEXT || text.contains('\n') {
        return Err(OutputError::Text);
    }
    Ok(OutputLine {
        service: service.to_owned(),
        stream,
        text: text.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::wire::FrameReader;

    fn line(service: &str, text: &str) -> OutputLine {
        OutputLine {
            service: service.to_owned(),
            stream: Stream::Err,
            text: text.to_owned(),
        }
    }

    #[test]
    fn lines_round_trip_and_long_text_is_cut_at_a_character() {
        let sample = line("logd", "Log service ready – ok");
        let frame = encode_frame(&sample).unwrap();
        let mut seen = Vec::new();
        FrameReader::new()
            .push(&frame, |body| seen.push(decode_body(body).unwrap()))
            .unwrap();
        assert_eq!(seen, vec![sample]);
        let long = "ä".repeat(400);
        let frame = encode_frame(&line("bus", &long)).unwrap();
        let back = decode_body(&frame[2..]).unwrap();
        assert!(back.text.len() <= MAX_TEXT && long.starts_with(&back.text));
    }

    #[test]
    fn bad_names_line_feeds_and_damaged_bodies_are_refused() {
        for name in ["", "Bus", "a/b", &"x".repeat(MAX_NAME + 1)] {
            assert_eq!(encode_frame(&line(name, "t")), Err(OutputError::Name));
        }
        assert_eq!(encode_frame(&line("a", "x\ny")), Err(OutputError::Text));
        assert_eq!(decode_body(&[]), Err(OutputError::Truncated));
        assert_eq!(decode_body(&[3, 1, b'a']), Err(OutputError::Stream));
        assert_eq!(decode_body(&[1, 5, b'a']), Err(OutputError::Truncated));
        assert_eq!(decode_body(&[1, 1, b'A']), Err(OutputError::Name));
        assert_eq!(decode_body(&[1, 1, b'a', 0xff]), Err(OutputError::Text));
        assert_eq!(decode_body(&[1, 1, b'a', b'\n']), Err(OutputError::Text));
        for error in [
            OutputError::Truncated,
            OutputError::Stream,
            OutputError::Name,
            OutputError::Text,
        ] {
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn random_bodies_never_panic_and_what_passes_is_valid() {
        // Fixed seed: the same inputs on every run.
        let mut state: u64 = 0x5eed_f00d_u64;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = (state % 64) as usize;
            let body: Vec<u8> = (0..len)
                .map(|i| (state.rotate_left(i as u32 * 5) & 0xff) as u8)
                .map(|b| if b % 3 == 0 { b'a' + b % 26 } else { b })
                .collect();
            if let Ok(line) = decode_body(&body) {
                assert!(valid_name(&line.service) && line.text.len() <= MAX_TEXT);
            }
        }
    }
}
