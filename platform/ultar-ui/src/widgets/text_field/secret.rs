//! Wiping the bytes of a password field: the text lives in one buffer reserved for the
//! longest allowed secret, so it never moves; it is overwritten when deleted and dropped.

use super::TextField;

impl TextField {
    /// Overwrites the whole content with zeros and empties it (password fields only; the
    /// buffer is kept).
    pub(super) fn zero_all(&mut self) {
        if !self.password {
            return;
        }
        let mut bytes = std::mem::take(&mut self.text).into_bytes();
        bytes.iter_mut().for_each(|b| *b = 0);
        bytes.clear();
        std::hint::black_box(&bytes);
        self.text = String::from_utf8(bytes).unwrap_or_default();
    }

    /// Overwrites the `removed` bytes behind the end that a deletion left in the buffer.
    pub(super) fn scrub_tail(&mut self, removed: usize) {
        if !self.password || removed == 0 {
            return;
        }
        let len = self.text.len();
        let mut bytes = std::mem::take(&mut self.text).into_bytes();
        bytes.resize(len + removed, 0);
        bytes.truncate(len);
        std::hint::black_box(&bytes);
        self.text = String::from_utf8(bytes).unwrap_or_default();
    }
}

impl Drop for TextField {
    fn drop(&mut self) {
        self.zero_all();
    }
}
