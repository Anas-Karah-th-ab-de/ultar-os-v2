//! QR code: a pairing or account code that a phone camera reads (boards "Account": 96 × 96,
//! radius 8). Encoding is done by the `qrcode` crate (error correction M); the toolkit only
//! draws the modules – always dark on a light ground with a quiet zone of four modules, in
//! both themes, because scanners need that contrast.

use super::Label;
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{ColorRole, Radius, Space, LIGHT};
use std::fmt;

/// Longest content a QR code widget takes, in bytes (a link or a pairing token).
pub const MAX_QR_BYTES: usize = 512;
/// Edge of the code in points (boards: 96).
const EDGE: f32 = 2.0 * Space::S48.points();
/// Light modules around the code (the QR standard asks for four).
const QUIET: usize = 4;

/// Why a QR code could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrCodeError {
    /// More than [`MAX_QR_BYTES`] bytes.
    TooLong(usize),
    /// The encoder refused the content.
    Encoding,
}

impl fmt::Display for QrCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QrCodeError::TooLong(n) => write!(f, "{n} bytes, at most {MAX_QR_BYTES}"),
            QrCodeError::Encoding => write!(f, "content cannot be encoded as a QR code"),
        }
    }
}

impl std::error::Error for QrCodeError {}

/// A QR code with its accessible name (role image).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrCode {
    /// Accessible name ("QR code of your Ultar number").
    pub name: Label,
    /// Modules per side.
    width: usize,
    /// Dark modules, row by row.
    dark: Vec<bool>,
}

impl QrCode {
    /// Encodes `data` (at most [`MAX_QR_BYTES`]).
    pub fn new(name: Label, data: &[u8]) -> Result<QrCode, QrCodeError> {
        if data.len() > MAX_QR_BYTES {
            return Err(QrCodeError::TooLong(data.len()));
        }
        let code = qrcode::QrCode::with_error_correction_level(data, qrcode::EcLevel::M)
            .map_err(|_| QrCodeError::Encoding)?;
        Ok(QrCode {
            name,
            width: code.width(),
            dark: code
                .to_colors()
                .into_iter()
                .map(|c| c == qrcode::Color::Dark)
                .collect(),
        })
    }

    /// Modules per side (without the quiet zone).
    pub fn modules(&self) -> usize {
        self.width
    }

    /// `true` for a dark module at column `x`, row `y`.
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && self.dark.get(y * self.width + x).copied().unwrap_or(false)
    }

    pub(crate) fn measure(&self) -> Size {
        Size::new(EDGE, EDGE)
    }

    /// Light ground with rounded corners, then the dark modules on whole device pixels.
    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect) {
        let box_ = Rect::new(rect.x, rect.y, EDGE.min(rect.w), EDGE.min(rect.h));
        let (ground, ink) = (LIGHT.get(ColorRole::Raised), LIGHT.get(ColorRole::Text));
        p.fill(box_, Some(Radius::Control), ground);
        let scale = p.env().scale;
        let cells = (self.width + 2 * QUIET) as f32;
        // Whole device pixels per module keep every module the same size.
        let module_px = ((box_.w * scale) / cells).floor().max(1.0);
        let module = module_px / scale;
        let code = module * self.width as f32;
        let left = (box_.x + (box_.w - code) / 2.0) * scale;
        let top = (box_.y + (box_.h - code) / 2.0) * scale;
        let (left, top) = (left.round() / scale, top.round() / scale);
        for y in 0..self.width {
            // Runs of dark modules are drawn as one rectangle.
            let mut x = 0;
            while x < self.width {
                if !self.is_dark(x, y) {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < self.width && self.is_dark(x, y) {
                    x += 1;
                }
                let run = Rect::new(
                    left + start as f32 * module,
                    top + y as f32 * module,
                    (x - start) as f32 * module,
                    module,
                );
                p.fill(run, None, ink);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_refuses_too_much() {
        let code = QrCode::new(Label::key("qr"), b"ultar:42").unwrap();
        assert_eq!(code.modules(), 21, "version 1");
        // The three finder patterns: dark corners.
        let w = code.modules();
        assert!(code.is_dark(0, 0) && code.is_dark(w - 1, 0) && code.is_dark(0, w - 1));
        assert!(!code.is_dark(w, 0));
        let long = vec![b'a'; MAX_QR_BYTES + 1];
        assert_eq!(
            QrCode::new(Label::key("qr"), &long),
            Err(QrCodeError::TooLong(MAX_QR_BYTES + 1))
        );
        assert!(!QrCodeError::Encoding.to_string().is_empty());
        assert!(QrCode::new(Label::key("qr"), &vec![0u8; MAX_QR_BYTES]).is_ok());
    }
}
