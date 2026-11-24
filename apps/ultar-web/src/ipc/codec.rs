//! Field encoding of the engine-host protocol: little-endian integers, `f32` bits and
//! strings with a 16-bit length. Reading never panics and never reads past the payload.

use super::DecodeError;

/// Longest string field (bytes).
pub const MAX_STRING: usize = 8 * 1024;

/// Builds a message payload.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// A message of `kind`.
    pub fn new(kind: u8) -> Writer {
        Writer {
            buf: vec![0, 0, 0, 0, kind],
        }
    }
    /// Appends a byte.
    pub fn u8(mut self, v: u8) -> Writer {
        self.buf.push(v);
        self
    }
    /// Appends a 16-bit value.
    pub fn u16(mut self, v: u16) -> Writer {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends a 32-bit value.
    pub fn u32(mut self, v: u32) -> Writer {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends a 64-bit value.
    pub fn u64(mut self, v: u64) -> Writer {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends a float.
    pub fn f32(self, v: f32) -> Writer {
        self.u32(v.to_bits())
    }
    /// Appends a string, cut at [`MAX_STRING`] bytes on a character boundary.
    pub fn str(self, v: &str) -> Writer {
        let mut end = v.len().min(MAX_STRING);
        while !v.is_char_boundary(end) {
            end -= 1;
        }
        let mut w = self.u16(end as u16);
        w.buf.extend_from_slice(&v.as_bytes()[..end]);
        w
    }
    /// The framed message (length prefix filled in).
    pub fn finish(mut self) -> Vec<u8> {
        let len = (self.buf.len() - 4) as u32;
        self.buf[..4].copy_from_slice(&len.to_le_bytes());
        self.buf
    }
}

/// Reads the fields of one payload.
#[derive(Debug)]
pub struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    /// A reader over a payload (without length and kind).
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.data.len() < n {
            return Err(DecodeError::Truncated);
        }
        let (head, tail) = self.data.split_at(n);
        self.data = tail;
        Ok(head)
    }
    /// A byte.
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    /// A boolean byte (0 or 1 only).
    pub fn flag(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::BadValue),
        }
    }
    /// A 16-bit value.
    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    /// A 32-bit value.
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// A 64-bit value.
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        let lo = u64::from(self.u32()?);
        let hi = u64::from(self.u32()?);
        Ok(lo | hi << 32)
    }
    /// A finite float.
    pub fn f32(&mut self) -> Result<f32, DecodeError> {
        let v = f32::from_bits(self.u32()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(DecodeError::BadValue)
        }
    }
    /// A UTF-8 string of at most [`MAX_STRING`] bytes.
    pub fn str(&mut self) -> Result<String, DecodeError> {
        let len = usize::from(self.u16()?);
        if len > MAX_STRING {
            return Err(DecodeError::TooLong);
        }
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::BadUtf8)
    }
    /// Fails if bytes are left over.
    pub fn end(&self) -> Result<(), DecodeError> {
        if self.data.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }
}
