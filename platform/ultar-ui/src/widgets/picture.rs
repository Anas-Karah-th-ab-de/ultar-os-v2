//! A picture whose pixels the application supplies: the page of a web view, a video or a
//! camera image. The pixels are device pixels drawn 1:1 from the node's top start corner;
//! where they do not reach, the node shows the `surface` colour. The node takes focus (the
//! application forwards keys to what the picture shows while it has the focus) but draws no
//! state of its own.

use super::Label;
use crate::canvas::Canvas;
use crate::geom::Rect;
use crate::paint::Painter;
use crate::tokens::ColorRole;
use std::fmt;
use std::rc::Rc;

/// An application-supplied picture.
#[derive(Clone)]
pub struct PixelArea {
    /// Accessible name (catalog key or data), e.g. the page title.
    pub name: Label,
    /// The current frame; `None` until the application sets one.
    pub(crate) frame: Option<Rc<Canvas>>,
}

impl PixelArea {
    /// A picture without a frame yet.
    pub fn new(name: Label) -> PixelArea {
        PixelArea { name, frame: None }
    }

    /// Width and height of the current frame in device pixels.
    pub fn frame_size(&self) -> Option<(u32, u32)> {
        self.frame.as_ref().map(|f| (f.width(), f.height()))
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect) {
        let ground = p.color(ColorRole::Surface);
        p.fill(rect, None, ground);
        if let Some(frame) = &self.frame {
            p.draw_canvas(frame, rect);
        }
    }
}

impl PartialEq for PixelArea {
    fn eq(&self, other: &PixelArea) -> bool {
        let same = match (&self.frame, &other.frame) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.name == other.name && same
    }
}

impl fmt::Debug for PixelArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixelArea")
            .field("name", &self.name)
            .field("frame", &self.frame_size())
            .finish()
    }
}
