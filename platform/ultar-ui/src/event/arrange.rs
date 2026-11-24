//! Dragging the items of an arrangement canvas (`widgets::Arrangement`).

use crate::event::{Action, ActionKind};
use crate::geom::Point;
use crate::ui::{NodeId, Ui, UiError};
use crate::widgets::{Arrangement, Widget};

impl Ui {
    fn canvas_mut(&mut self, id: NodeId) -> Option<&mut Arrangement> {
        match self.nodes.get_mut(id.index()).map(|n| &mut n.widget) {
            Some(Widget::Arrangement(canvas)) => Some(canvas),
            _ => None,
        }
    }

    /// A press on item `part` of a canvas: remember where it was grabbed.
    pub(crate) fn grab_item(&mut self, id: NodeId, part: Option<usize>, pos: Point) {
        let rect = self.rect(id);
        let Some(canvas) = self.canvas_mut(id) else {
            return;
        };
        canvas.grab = match (part, rect) {
            (Some(index), Some(rect)) => canvas
                .item_rect(rect, index)
                .map(|item| (index, Point::new(pos.x - item.x, pos.y - item.y), false)),
            _ => None,
        };
    }

    /// The pointer moved with a grabbed item: it follows (inside the canvas, snapping to
    /// the edges of the others). Returns `true` when an item is being dragged.
    pub(crate) fn drag_item(&mut self, id: NodeId, pos: Point) -> bool {
        let Some(rect) = self.rect(id) else {
            return false;
        };
        let Some(canvas) = self.canvas_mut(id) else {
            return false;
        };
        let Some((index, offset, _)) = canvas.grab else {
            return false;
        };
        let corner = Point::new(pos.x - offset.x - rect.x, pos.y - offset.y - rect.y);
        if canvas.place(index, corner, rect.w) {
            canvas.grab = Some((index, offset, true));
            self.touch(id);
        }
        true
    }

    /// The press ended: an item that moved reports `Arranged(index)`.
    pub(crate) fn release_item(&mut self, id: NodeId, out: &mut Vec<Action>) {
        if let Some((index, _, true)) = self.canvas_mut(id).and_then(|c| c.grab.take()) {
            out.push(Action {
                node: id,
                kind: ActionKind::Arranged(index),
            });
        }
        if let Some(canvas) = self.canvas_mut(id) {
            canvas.grab = None;
        }
    }

    /// Where item `index` of a canvas is (points from the canvas' top-left corner).
    pub fn arranged_rect(&self, id: NodeId, index: usize) -> Result<crate::geom::Rect, UiError> {
        match &self.node(id)?.widget {
            Widget::Arrangement(canvas) => canvas
                .items
                .get(index)
                .map(|i| i.rect)
                .ok_or(UiError::OptionOutOfRange { node: id, index }),
            _ => Err(UiError::WrongWidget {
                node: id,
                expected: "arrangement",
            }),
        }
    }
}
