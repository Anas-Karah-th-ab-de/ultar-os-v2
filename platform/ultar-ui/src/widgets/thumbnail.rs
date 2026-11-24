//! A small picture shown in place of an icon (file thumbnails in a grid of files).

use std::fmt;
use std::rc::Rc;

/// Longest edge of a thumbnail in pixels.
pub const MAX_THUMBNAIL_EDGE: u32 = 256;

/// A decoded picture: straight-alpha RGBA, at most [`MAX_THUMBNAIL_EDGE`] on each edge.
/// Cloning shares the pixels; two thumbnails are equal when they share them.
#[derive(Clone)]
pub struct Thumbnail {
    width: u32,
    height: u32,
    rgba: Rc<[u8]>,
}

impl Thumbnail {
    /// A thumbnail of `width` × `height` pixels; `None` when a size is 0 or too large or
    /// the length of `rgba` does not match.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Thumbnail> {
        let ok = (1..=MAX_THUMBNAIL_EDGE).contains(&width)
            && (1..=MAX_THUMBNAIL_EDGE).contains(&height)
            && rgba.len() == width as usize * height as usize * 4;
        ok.then(|| Thumbnail {
            width,
            height,
            rgba: rgba.into(),
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels (RGBA, rows top to bottom).
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

impl PartialEq for Thumbnail {
    fn eq(&self, other: &Thumbnail) -> bool {
        self.width == other.width
            && self.height == other.height
            && Rc::ptr_eq(&self.rgba, &other.rgba)
    }
}

impl Eq for Thumbnail {}

impl fmt::Debug for Thumbnail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Thumbnail({}x{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_checked_and_equality_is_by_pixels_shared() {
        assert!(Thumbnail::new(0, 1, vec![]).is_none());
        assert!(Thumbnail::new(2, 2, vec![0; 15]).is_none());
        assert!(Thumbnail::new(MAX_THUMBNAIL_EDGE + 1, 1, vec![0; 1028]).is_none());
        let a = Thumbnail::new(1, 1, vec![1, 2, 3, 4]).unwrap();
        let b = Thumbnail::new(1, 1, vec![1, 2, 3, 4]).unwrap();
        assert_eq!(a, a.clone());
        assert_ne!(a, b);
        assert_eq!((a.width(), a.height(), a.rgba()), (1, 1, &[1, 2, 3, 4][..]));
        assert_eq!(format!("{a:?}"), "Thumbnail(1x1)");
    }
}
