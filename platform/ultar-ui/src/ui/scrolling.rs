//! Scroll containers in the tree: offsets, clipping, and keeping the focus in view.

use super::{NodeId, Ui, UiError};
use crate::event::{Action, ActionKind};
use crate::geom::{Point, Rect};
use crate::tokens::Space;
use crate::widgets::{focus_outset, Widget};

impl Ui {
    /// `true` for a scroll container whose content is taller than its viewport.
    pub(crate) fn scrollable(&self, id: NodeId) -> bool {
        let node = &self.nodes[id.index()];
        matches!(&node.widget, Widget::Scroll(s) if s.max_offset(node.frame.height) > 0.0)
    }

    /// `true` for nodes the user operates: interactive widgets, and scroll containers
    /// while they can scroll (so that the keyboard can reach and move them).
    pub(crate) fn interactive(&self, id: NodeId) -> bool {
        self.nodes[id.index()].widget.is_interactive() || self.scrollable(id)
    }

    /// The scroll offset of a scroll container in points.
    pub fn scroll_offset(&self, id: NodeId) -> Result<f32, UiError> {
        Ok(self.scroller(id)?.offset)
    }

    /// Scrolls a container to `offset` points (clamped to its content) without emitting an
    /// action.
    pub fn set_scroll_offset(&mut self, id: NodeId, offset: f32) -> Result<(), UiError> {
        self.scroller(id)?;
        self.layout();
        self.scroll_to(id, offset, &mut Vec::new());
        Ok(())
    }

    /// Scrolls every scroll container above `id` so that the node is inside its viewport.
    pub fn scroll_into_view(&mut self, id: NodeId) -> Result<(), UiError> {
        self.node(id)?;
        self.layout();
        self.reveal(id, &mut Vec::new());
        Ok(())
    }

    /// Moves a scroll container to `offset` (clamped). Returns `true` when it moved; then
    /// the viewport is repainted, floating things anchored to its content close, and
    /// `Scrolled` is pushed to `out`.
    pub(crate) fn scroll_to(&mut self, id: NodeId, offset: f32, out: &mut Vec<Action>) -> bool {
        let height = self.nodes[id.index()].frame.height;
        let Ok(scroll) = self.scroller_mut(id) else {
            return false;
        };
        let target = scroll.clamp(offset, height);
        if target == scroll.offset {
            return false;
        }
        scroll.offset = target;
        self.touch(id);
        self.hide_tooltip();
        self.close_popup(out);
        self.close_menu_with(out);
        out.push(Action {
            node: id,
            kind: ActionKind::Scrolled(target),
        });
        true
    }

    /// Scrolls the containers above `id` so that it is visible, the innermost first.
    pub(crate) fn reveal(&mut self, id: NodeId, out: &mut Vec<Action>) {
        let margin = focus_outset().max(Space::S4.points());
        let mut above = self.nodes[id.index()].clip_parent;
        while let Some(scroller) = above {
            above = self.nodes[scroller.index()].clip_parent;
            let (Some(target), Some(view), Ok(scroll)) =
                (self.rect(id), self.rect(scroller), self.scroller(scroller))
            else {
                continue;
            };
            let over_top = view.y - (target.y - margin);
            let over_bottom = (target.bottom() + margin) - view.bottom();
            let offset = scroll.offset;
            if over_top > 0.0 || target.h + 2.0 * margin > view.h {
                // Above the viewport, or taller than it: show its beginning.
                self.scroll_to(scroller, offset - over_top, out);
            } else if over_bottom > 0.0 {
                self.scroll_to(scroller, offset + over_bottom, out);
            }
        }
    }

    /// The rectangle a node is drawn and hit in: the intersection of the scroll containers
    /// and dialog above it. `None` when nothing clips it.
    pub(crate) fn clip_rect(&self, id: NodeId) -> Option<Rect> {
        let mut clip: Option<Rect> = None;
        let mut above = self.nodes.get(id.index())?.clip_parent;
        while let Some(ancestor) = above {
            let rect = self.rect(ancestor)?;
            clip = Some(clip.map_or(rect, |c| c.intersect(&rect)));
            above = self.nodes[ancestor.index()].clip_parent;
        }
        clip
    }

    /// `true` when `pos` lies in the visible part of `id`.
    pub(crate) fn visible_at(&self, id: NodeId, pos: Point) -> bool {
        self.rect(id).is_some_and(|r| r.contains(pos))
            && self.clip_rect(id).is_none_or(|clip| clip.contains(pos))
    }

    /// The scroll containers under `pos` that can scroll, the innermost first. Only nodes of
    /// the topmost dialog count while one is open.
    pub(crate) fn scrollers_at(&self, pos: Point) -> Vec<NodeId> {
        self.reachable()
            .iter()
            .rev()
            .copied()
            .filter(|id| self.scrollable(*id) && self.visible_at(*id, pos))
            .collect()
    }

    /// The nearest scroll container that can scroll, starting at `id` itself.
    pub(crate) fn scroller_of(&self, id: NodeId) -> Option<NodeId> {
        let mut current = Some(id);
        while let Some(node) = current {
            if self.scrollable(node) {
                return Some(node);
            }
            current = self.nodes[node.index()].clip_parent;
        }
        None
    }
}
