//! Setting the pixels of a [`Picture`](crate::widgets::Picture) node.

use super::{NodeId, Ui, UiError};
use crate::canvas::Canvas;
use crate::geom::{PixelRect, Rect};
use crate::widgets::Widget;
use std::rc::Rc;

impl Ui {
    fn picture_frame(&mut self, id: NodeId) -> Result<&mut Option<Rc<Canvas>>, UiError> {
        match &mut self.node_mut(id)?.widget {
            Widget::Picture(picture) => Ok(&mut picture.frame),
            _ => Err(UiError::WrongWidget {
                node: id,
                expected: "picture",
            }),
        }
    }

    /// Shows `frame` (device pixels) in picture `id`; the node is repainted.
    pub fn set_picture(&mut self, id: NodeId, frame: Canvas) -> Result<(), UiError> {
        *self.picture_frame(id)? = Some(Rc::new(frame));
        self.touch(id);
        Ok(())
    }

    /// Removes the frame of picture `id` (it shows the surface colour).
    pub fn clear_picture(&mut self, id: NodeId) -> Result<(), UiError> {
        *self.picture_frame(id)? = None;
        self.touch(id);
        Ok(())
    }

    /// Writes `area` (device pixels of the frame) from BGRX/BGRA `rows` with `stride` bytes
    /// per row into the frame of picture `id`; only that part of the node is repainted.
    /// Fails without a frame or when the rows are too short (nothing is changed then).
    pub fn write_picture(
        &mut self,
        id: NodeId,
        area: PixelRect,
        rows: &[u8],
        stride: usize,
    ) -> Result<(), UiError> {
        let frame = self.picture_frame(id)?;
        let Some(frame) = frame.as_mut() else {
            return Err(UiError::WrongWidget {
                node: id,
                expected: "picture with a frame",
            });
        };
        Rc::make_mut(frame)
            .write_bgrx(area, rows, stride)
            .map_err(|_| UiError::InvalidSize)?;
        Rc::make_mut(frame).take_damage();
        let Some(rect) = self.rect(id) else {
            return Ok(());
        };
        if self.layers.backdrop.ready {
            self.touch(id);
            return Ok(());
        }
        let s = self.env.scale;
        let changed = Rect::new(
            rect.x + area.x0 as f32 / s,
            rect.y + area.y0 as f32 / s,
            area.width().max(0) as f32 / s + 1.0,
            area.height().max(0) as f32 / s + 1.0,
        );
        self.damage(changed.intersect(&rect));
        Ok(())
    }

    /// Size of the frame of picture `id` in device pixels.
    pub fn picture_size(&self, id: NodeId) -> Option<(u32, u32)> {
        match self.widget(id)? {
            Widget::Picture(picture) => picture.frame_size(),
            _ => None,
        }
    }
}
