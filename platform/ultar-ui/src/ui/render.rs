//! Painting the tree: only damaged regions are redrawn.

use super::{DragKind, NodeId, PaintStep, Ui};
use crate::canvas::Canvas;
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{size, state, Radius, Space};
use crate::widgets::{
    focus_outset, paint_tooltip, shadow_reach, PaintCx, PopupView, Select, Widget,
};

impl Ui {
    /// Geometry of the open options list, if one is open.
    pub(crate) fn popup_view(&self) -> Option<(NodeId, PopupView)> {
        let popup = self.input.popup?;
        let rect = self.rect(popup.select)?;
        let select = self.select(popup.select).ok()?;
        let field = Select::field(rect, self.mode);
        Some((
            popup.select,
            select.popup(field, self.size(), self.mode, popup.scroll),
        ))
    }

    /// Marks the open options list (with its shadow) for repaint.
    pub(crate) fn touch_popup(&mut self) {
        if let Some((_, view)) = self.popup_view() {
            self.damage_floating(view.rect);
        }
    }

    /// The area a node may draw into: its rectangle plus focus ring, halo or shadow.
    fn ink_bounds(&self, id: NodeId, rect: Rect) -> Rect {
        match &self.nodes[id.index()].widget {
            Widget::Card(card) if card.floating => {
                let reach = shadow_reach();
                let grown = rect.inflate(reach.x);
                Rect::new(grown.x, grown.y, grown.w, grown.h + reach.y)
            }
            // The scrim of a dialog covers the whole surface.
            Widget::Dialog(_) => Rect::new(0.0, 0.0, self.size().w, self.size().h),
            _ if self.interactive(id) => rect.inflate(focus_outset().max(Space::S4.points())),
            _ => rect,
        }
    }

    /// Width and size of the hosted child of a row-like widget (zero without one).
    fn hosted(&self, id: NodeId) -> Size {
        self.nodes[id.index()]
            .children
            .first()
            .map_or(Size::default(), |c| {
                let frame = self.nodes[c.index()].frame;
                Size::new(frame.width, frame.height)
            })
    }

    fn paint_widget(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, id: NodeId, rect: Rect) {
        let node = &self.nodes[id.index()];
        let v = self.visual(id);
        let error = node.error.as_ref();
        let parts = (
            self.input
                .hover_part
                .filter(|_| self.input.hover == Some(id)),
            self.input.pressed_part(id),
        );
        match &node.widget {
            Widget::Flex(_) | Widget::List(_) | Widget::Scroll(_) => {}
            Widget::Text(w) => w.paint(p, cx, rect),
            Widget::Icon(w) => w.paint(p, rect),
            Widget::Button(w) => w.paint(p, cx, rect, &v),
            Widget::IconButton(w) => w.paint(p, cx, rect, &v),
            Widget::Switch(w) => w.paint(p, cx, rect, &v, self.knob_position(w)),
            Widget::Select(w) => w.paint(p, cx, rect, &v, error),
            Widget::Slider(w) => w.paint(p, rect, &v),
            Widget::TextField(w) => {
                let has_focus = self.input.focus == Some(id) && !v.disabled;
                w.paint(p, cx, rect, &v, has_focus, error);
            }
            Widget::Row(w) => w.paint(p, cx, rect, &v, self.hosted(id).w),
            Widget::Card(w) => w.paint(p, rect),
            Widget::Badge(w) => w.paint(p, cx, rect),
            Widget::Progress(w) => w.paint(p, rect, cx.spin),
            Widget::Checkbox(w) => w.paint(p, cx, rect, &v),
            Widget::RadioGroup(w) => w.paint(p, cx, rect, &v, parts),
            Widget::Tabs(w) => w.paint(p, cx, rect, &v, parts),
            Widget::InfoBar(w) => w.paint(p, cx, rect, self.hosted(id).w),
            Widget::EmptyState(w) => w.paint(p, cx, rect, self.hosted(id)),
            Widget::ListItem(w) => w.paint(p, cx, rect, &v, self.hosted(id).w),
            Widget::Dialog(w) => {
                w.paint(p, rect, Rect::new(0.0, 0.0, self.size().w, self.size().h))
            }
        }
    }

    fn paint_node(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, id: NodeId, region: &Rect) {
        let Some(rect) = self.rect(id) else {
            return;
        };
        if !self.ink_bounds(id, rect).intersects(region) {
            return;
        }
        match self.clip_rect(id) {
            Some(clip) if !clip.intersects(region) => {}
            Some(clip) => {
                let saved = p.clip(clip);
                self.paint_widget(p, cx, id, rect);
                p.restore_clip(saved);
            }
            None => self.paint_widget(p, cx, id, rect),
        }
    }

    /// Paints the scrollbar of a scroll container and, with keyboard focus, its focus ring
    /// (inside the viewport, where it cannot be clipped away).
    fn paint_scrollbar(&self, p: &mut Painter<'_>, id: NodeId, region: &Rect) {
        let (Some(view), Ok(scroll)) = (self.rect(id), self.scroller(id)) else {
            return;
        };
        if !view.intersects(region) {
            return;
        }
        let v = self.visual(id);
        // A finger that only rests on the content has not grabbed anything yet.
        let dragged = self.layers.drag.is_some_and(|d| {
            d.scroller == id && !matches!(d.kind, DragKind::Finger { active: false, .. })
        });
        let clip = self.clip_rect(id).map_or(view, |c| c.intersect(&view));
        let saved = p.clip(clip);
        scroll.paint_bar(p, view, v.hovered || v.pressed || dragged);
        if v.focused {
            let ring = p.color(state::FOCUS_RING);
            p.stroke(view, Some(Radius::Control), size::FOCUS_RING, ring);
        }
        p.restore_clip(saved);
    }

    fn paint_region(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, region: Rect) {
        let saved = p.clip(region);
        let background = p.color(self.background());
        p.fill(region, None, background);
        for step in &self.layers.steps {
            match step {
                PaintStep::Node(id) => self.paint_node(p, cx, *id, &region),
                PaintStep::ScrollBar(id) => self.paint_scrollbar(p, *id, &region),
            }
        }
        if let (Some(popup), Some((id, view))) = (self.input.popup, self.popup_view()) {
            if let Ok(select) = self.select(id) {
                select.paint_popup(p, cx, &view, popup.highlight);
            }
        }
        self.paint_menu(p, cx);
        if let Some(tip) = self.layers.tip {
            if let Some(label) = self.nodes[tip.node.index()].tooltip.as_ref() {
                paint_tooltip(p, cx, tip.rect, label);
            }
        }
        p.restore_clip(saved);
    }

    /// Paints what changed since the last call into `canvas` (everything on the first call
    /// or after a layout change). The canvas records the touched pixels in its damage list.
    pub fn paint(&mut self, canvas: &mut Canvas) {
        self.layout();
        let surface = Rect::new(0.0, 0.0, self.size().w, self.size().h);
        let regions: Vec<Rect> = if self.dirty.full {
            self.dirty.rects.clear();
            vec![surface]
        } else {
            std::mem::take(&mut self.dirty.rects)
        };
        self.dirty.full = false;
        if regions.is_empty() {
            return;
        }
        // Nothing else borrows the resources while a frame is painted; if that ever were
        // the case the frame is skipped instead of panicking.
        let Ok(mut resources) = self.res.try_borrow_mut() else {
            self.dirty.full = true;
            return;
        };
        let mut painter = Painter::new(canvas, &mut resources, self.env);
        let cx = PaintCx {
            tr: &self.tr,
            mode: self.mode,
            spin: self.spin(),
        };
        for region in regions {
            let region = snap_out(region.intersect(&surface), self.env.scale);
            if !region.is_empty() {
                self.paint_region(&mut painter, &cx, region);
            }
        }
    }

    /// Paints the whole surface regardless of what changed.
    pub fn paint_all(&mut self, canvas: &mut Canvas) {
        self.repaint_all();
        self.paint(canvas);
    }
}

/// Grows a logical rectangle so that its edges fall on device pixels.
fn snap_out(rect: Rect, scale: f32) -> Rect {
    let x0 = (rect.x * scale).floor() / scale;
    let y0 = (rect.y * scale).floor() / scale;
    let x1 = (rect.right() * scale).ceil() / scale;
    let y1 = (rect.bottom() * scale).ceil() / scale;
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
