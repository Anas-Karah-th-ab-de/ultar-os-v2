//! Byte-level encoding of the protocol: little-endian integers, bounded
//! strings and byte blocks, and the message frame.
//!
//! Frame = `u32 body length | u16 opcode | u16 reserved (0)` followed by the
//! body. Nothing is trusted: every length is checked against the remaining
//! input and against a fixed limit before anything is allocated.

use std::fmt;

/// Bytes of the frame header.
pub const HEADER: usize = 8;
/// Largest body of any message: one inline pixel block of 1 MiB plus its
/// fields.
pub const MAX_BODY: usize = (1 << 20) + 64;
/// Largest string in any message (titles are capped lower by their
/// decoders).
pub const MAX_STRING: usize = 4096;

/// Why a message could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// The body ended before a field was complete.
    Truncated,
    /// The body has bytes after the last field.
    Trailing,
    /// A length is above its limit.
    TooLong,
    /// A string is not UTF-8 or contains control characters.
    BadText,
    /// An enum value, flag or opcode is not known.
    BadValue,
    /// The frame header is invalid (reserved bits set).
    BadHeader,
}

impl WireError {
    /// Stable key of the error.
    pub const fn key(self) -> &'static str {
        match self {
            WireError::Truncated => "surface.wire.truncated",
            WireError::Trailing => "surface.wire.trailing",
            WireError::TooLong => "surface.wire.too_long",
            WireError::BadText => "surface.wire.bad_text",
            WireError::BadValue => "surface.wire.bad_value",
            WireError::BadHeader => "surface.wire.bad_header",
        }
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

impl std::error::Error for WireError {}

/// Builds one message.
#[derive(Debug)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// Starts a message with `opcode`; the length is filled in by
    /// [`Writer::finish`].
    pub fn new(opcode: u16) -> Writer {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&[0, 0, 0, 0]);
        buf.extend_from_slice(&opcode.to_le_bytes());
        buf.extend_from_slice(&[0, 0]);
        Writer { buf }
    }

    /// Appends a byte.
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    /// Appends a bool as one byte.
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }

    /// Appends a `u16`.
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// Appends a `u32`.
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// Appends an `i32`.
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// Appends a `u64`.
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// Appends a string (`u16` length + bytes), cut at `limit` bytes on a
    /// character boundary.
    pub fn str(&mut self, v: &str, limit: usize) -> &mut Self {
        let mut end = v.len().min(limit).min(MAX_STRING);
        while !v.is_char_boundary(end) {
            end -= 1;
        }
        self.u16(end as u16);
        self.buf.extend_from_slice(&v.as_bytes()[..end]);
        self
    }

    /// Appends a byte block (`u32` length + bytes).
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v);
        self
    }

    /// Completes the frame. Bodies above [`MAX_BODY`] are the caller's bug
    /// and are refused with [`WireError::TooLong`].
    pub fn finish(mut self) -> Result<Vec<u8>, WireError> {
        let body = self.buf.len() - HEADER;
        if body > MAX_BODY {
            return Err(WireError::TooLong);
        }
        self.buf[0..4].copy_from_slice(&(body as u32).to_le_bytes());
        Ok(self.buf)
    }
}

/// Reads the fields of one message body.
#[derive(Debug)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader over a body.
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(n).ok_or(WireError::Truncated)?;
        let slice = self.data.get(self.pos..end).ok_or(WireError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    /// Reads a byte.
    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    /// Reads a bool (0 or 1, anything else is refused).
    pub fn bool(&mut self) -> Result<bool, WireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WireError::BadValue),
        }
    }

    /// Reads a `u16`.
    pub fn u16(&mut self) -> Result<u16, WireError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Reads a `u32`.
    pub fn u32(&mut self) -> Result<u32, WireError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Reads an `i32`.
    pub fn i32(&mut self) -> Result<i32, WireError> {
        Ok(self.u32()? as i32)
    }

    /// Reads a `u64`.
    pub fn u64(&mut self) -> Result<u64, WireError> {
        let lo = u64::from(self.u32()?);
        let hi = u64::from(self.u32()?);
        Ok(lo | hi << 32)
    }

    /// Reads a string of at most `limit` bytes; control characters other
    /// than none are refused (titles, identifiers and MIME types never
    /// contain them).
    pub fn str(&mut self, limit: usize) -> Result<String, WireError> {
        let len = usize::from(self.u16()?);
        if len > limit.min(MAX_STRING) {
            return Err(WireError::TooLong);
        }
        let text = std::str::from_utf8(self.take(len)?).map_err(|_| WireError::BadText)?;
        if text.chars().any(char::is_control) {
            return Err(WireError::BadText);
        }
        Ok(text.to_owned())
    }

    /// Reads free text (input method commits) of at most `limit` bytes;
    /// newline and tab are allowed, other control characters are not.
    pub fn text(&mut self, limit: usize) -> Result<String, WireError> {
        let len = usize::from(self.u16()?);
        if len > limit.min(MAX_STRING) {
            return Err(WireError::TooLong);
        }
        let text = std::str::from_utf8(self.take(len)?).map_err(|_| WireError::BadText)?;
        if text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(WireError::BadText);
        }
        Ok(text.to_owned())
    }

    /// Reads a byte block of at most `limit` bytes without copying.
    pub fn bytes(&mut self, limit: usize) -> Result<&'a [u8], WireError> {
        let len = self.u32()? as usize;
        if len > limit {
            return Err(WireError::TooLong);
        }
        self.take(len)
    }

    /// Bytes not read yet (an extended message may carry optional trailing fields).
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Fails if bytes are left over.
    pub fn end(&self) -> Result<(), WireError> {
        if self.pos == self.data.len() {
            Ok(())
        } else {
            Err(WireError::Trailing)
        }
    }
}

/// A decoded frame header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Length of the body.
    pub len: usize,
    /// Message type.
    pub opcode: u16,
}

/// Parses a frame header; the body length is checked against [`MAX_BODY`].
pub fn parse_header(bytes: &[u8; HEADER]) -> Result<Header, WireError> {
    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let opcode = u16::from_le_bytes([bytes[4], bytes[5]]);
    if bytes[6] != 0 || bytes[7] != 0 {
        return Err(WireError::BadHeader);
    }
    if len > MAX_BODY {
        return Err(WireError::TooLong);
    }
    Ok(Header { len, opcode })
}

/// Splits a byte stream into frames. Holds at most one incomplete frame
/// (bounded by [`MAX_BODY`] + [`HEADER`]).
#[derive(Debug, Default)]
pub struct FrameReader {
    buf: Vec<u8>,
    start: usize,
}

impl FrameReader {
    /// An empty reader.
    pub fn new() -> FrameReader {
        FrameReader::default()
    }

    /// Room the reader accepts now without exceeding its bound.
    pub fn room(&self) -> usize {
        (MAX_BODY + HEADER) * 2 - (self.buf.len() - self.start)
    }

    /// Appends received bytes. The caller reads at most [`FrameReader::room`].
    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > 0 && self.start == self.buf.len() {
            self.buf.clear();
            self.start = 0;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// The next complete frame as (opcode, body), or `None` if more bytes
    /// are needed.
    pub fn next_frame(&mut self) -> Result<Option<(u16, Vec<u8>)>, WireError> {
        let pending = &self.buf[self.start..];
        if pending.len() < HEADER {
            self.compact();
            return Ok(None);
        }
        let mut head = [0u8; HEADER];
        head.copy_from_slice(&pending[..HEADER]);
        let header = parse_header(&head)?;
        if pending.len() < HEADER + header.len {
            self.compact();
            return Ok(None);
        }
        let body = pending[HEADER..HEADER + header.len].to_vec();
        self.start += HEADER + header.len;
        Ok(Some((header.opcode, body)))
    }

    /// Bytes buffered but not yet returned.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.start
    }

    fn compact(&mut self) {
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_of_every_field_kind() {
        let mut w = Writer::new(7);
        w.u8(1).bool(true).u16(2).u32(3).i32(-4).u64(u64::MAX - 1);
        w.str("عنوان", 64).bytes(&[9, 8]);
        let frame = w.finish().unwrap();
        let mut head = [0u8; HEADER];
        head.copy_from_slice(&frame[..HEADER]);
        let header = parse_header(&head).unwrap();
        assert_eq!(header.opcode, 7);
        assert_eq!(header.len, frame.len() - HEADER);
        let mut r = Reader::new(&frame[HEADER..]);
        assert_eq!(r.u8().unwrap(), 1);
        assert!(r.bool().unwrap());
        assert_eq!(r.u16().unwrap(), 2);
        assert_eq!(r.u32().unwrap(), 3);
        assert_eq!(r.i32().unwrap(), -4);
        assert_eq!(r.u64().unwrap(), u64::MAX - 1);
        assert_eq!(r.str(64).unwrap(), "عنوان");
        assert_eq!(r.bytes(2).unwrap(), &[9, 8]);
        r.end().unwrap();
    }

    #[test]
    fn strings_are_cut_on_char_boundaries_and_checked() {
        let mut w = Writer::new(1);
        w.str("ab\u{0627}", 3);
        let frame = w.finish().unwrap();
        let mut r = Reader::new(&frame[HEADER..]);
        assert_eq!(r.str(3).unwrap(), "ab");
        let mut bad = Writer::new(1);
        bad.u16(2).u8(0xff).u8(0xfe);
        let frame = bad.finish().unwrap();
        assert_eq!(
            Reader::new(&frame[HEADER..]).str(8),
            Err(WireError::BadText)
        );
        let mut ctrl = Writer::new(1);
        ctrl.str("a\u{7}", 8);
        let frame = ctrl.finish().unwrap();
        assert_eq!(
            Reader::new(&frame[HEADER..]).str(8),
            Err(WireError::BadText)
        );
        assert_eq!(
            Reader::new(&frame[HEADER..]).str(1),
            Err(WireError::TooLong)
        );
        let mut nl = Writer::new(1);
        nl.str("a\nb", 8);
        let frame = nl.finish().unwrap();
        assert_eq!(Reader::new(&frame[HEADER..]).text(8).unwrap(), "a\nb");
    }

    #[test]
    fn malformed_input_is_refused() {
        let mut r = Reader::new(&[1, 2]);
        assert_eq!(r.u32(), Err(WireError::Truncated));
        assert_eq!(Reader::new(&[2]).bool(), Err(WireError::BadValue));
        assert_eq!(Reader::new(&[1]).end(), Err(WireError::Trailing));
        assert_eq!(
            Reader::new(&[255, 255, 255, 255]).bytes(10),
            Err(WireError::TooLong)
        );
        assert_eq!(
            parse_header(&[0, 0, 0, 0, 1, 0, 1, 0]),
            Err(WireError::BadHeader)
        );
        assert_eq!(
            parse_header(&[255, 255, 255, 255, 1, 0, 0, 0]),
            Err(WireError::TooLong)
        );
        let mut big = Writer::new(1);
        big.bytes(&vec![0u8; MAX_BODY]);
        assert_eq!(big.finish().err(), Some(WireError::TooLong));
        assert!(!WireError::Truncated.to_string().is_empty());
    }

    #[test]
    fn frames_split_across_reads() {
        let mut w = Writer::new(3);
        w.u32(42);
        let frame = w.finish().unwrap();
        let mut both = frame.clone();
        both.extend_from_slice(&frame);
        let mut fr = FrameReader::new();
        assert!(fr.room() > MAX_BODY);
        for chunk in both.chunks(5) {
            fr.push(chunk);
        }
        assert_eq!(
            fr.next_frame().unwrap(),
            Some((3, 42u32.to_le_bytes().to_vec()))
        );
        assert_eq!(
            fr.next_frame().unwrap(),
            Some((3, 42u32.to_le_bytes().to_vec()))
        );
        assert_eq!(fr.next_frame().unwrap(), None);
        assert_eq!(fr.buffered(), 0);
        fr.push(&frame[..6]);
        assert_eq!(fr.next_frame().unwrap(), None);
        fr.push(&frame[6..]);
        assert!(fr.next_frame().unwrap().is_some());
        fr.push(&[0, 0, 0, 0, 0, 0, 9, 9]);
        assert_eq!(fr.next_frame(), Err(WireError::BadHeader));
    }
}
