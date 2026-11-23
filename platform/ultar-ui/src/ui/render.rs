//! Painting the tree: only damaged regions are redrawn.

use super::{DragKind, NodeId, PaintStep, Ui};
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{size, state, Radius, Space};
use crate::widgets::{
    focus_outset, paint_tooltip, shadow_reach, PaintCx, PopupView, Select, ToastView, Widget,
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
            Widget::Dialog(_) => {
                let reach = shadow_reach();
                let grown = rect.inflate(reach.x);
                Rect::new(grown.x, grown.y, grown.w, grown.h + reach.y)
            }
            // Glyph side bearings and anti-aliased edges may pass the rectangle slightly.
            _ => rect.inflate(focus_outset().max(Space::S4.points())),
        }
    }

    /// `true` for an item of a sidebar's navigation list.
    pub(crate) fn in_navigation(&self, id: NodeId) -> bool {
        self.parent(id)
            .is_some_and(|p| matches!(self.widget(p), Some(Widget::List(l)) if l.navigation))
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
            Widget::Slider(w) => w.paint(p, cx, rect, &v, error),
            Widget::TextField(w) => {
                let has_focus = self.input.focus == Some(id) && !v.disabled;
                w.paint(p, cx, rect, &v, has_focus, error);
            }
            Widget::Row(w) => w.paint(p, cx, rect, &v, self.hosted(id).w),
            Widget::Card(w) => w.paint(p, rect),
            Widget::Badge(w) => w.paint(p, cx, rect),
            Widget::Progress(w) => w.paint(p, rect, cx.spin),
            Widget::Checkbox(w) => w.paint(p, cx, rect, &v, error),
            Widget::RadioGroup(w) => w.paint(p, cx, rect, &v, parts),
            Widget::Tabs(w) => w.paint(p, cx, rect, &v, parts),
            Widget::InfoBar(w) => w.paint(p, cx, rect, self.hosted(id).w),
            Widget::EmptyState(w) => w.paint(p, cx, rect, self.hosted(id)),
            Widget::ListItem(w) => {
                let nav = self.in_navigation(id);
                w.paint(p, cx, rect, &v, (self.hosted(id).w, nav, error));
            }
            Widget::Table(w) => w.paint(p, cx, rect, &v, parts.0),
            Widget::Tree(w) => w.paint(p, cx, rect, &v, parts.0),
            Widget::HeaderBar(w) => w.paint(p, cx, rect),
            Widget::Dialog(w) => w.paint(p, rect),
        }
    }

    fn paint_node(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, id: NodeId, region: &Rect) {
        let Some(rect) = self.rect(id) else {
            return;
        };
        let ink = self.ink_bounds(id, rect);
        if !ink.intersects(region) {
            return;
        }
        // A widget never draws outside its ink bounds (a squeezed field drawing its icon
        // beside it would leave pixels no damage rectangle accounts for).
        let clip = self.clip_rect(id).map_or(ink, |c| c.intersect(&ink));
        if clip.intersects(region) {
            let saved = p.clip(clip);
            self.paint_widget(p, cx, id, rect);
            p.restore_clip(saved);
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

    /// Background and the paint steps before `end` (everything under the scrim, or the
    /// whole tree when `end` is the number of steps).
    pub(crate) fn paint_under(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        region: Rect,
        end: usize,
    ) {
        let background = p.color(self.background());
        p.fill(region, None, background);
        for step in self.layers.steps.iter().take(end) {
            self.paint_step(p, cx, *step, &region);
        }
    }

    /// The paint steps from `from` on, then the floating layers: options list, menu,
    /// toasts and tooltip.
    pub(crate) fn paint_top(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        region: Rect,
        (from, toasts): (usize, &[(usize, ToastView)]),
    ) {
        for step in self.layers.steps.iter().skip(from) {
            self.paint_step(p, cx, *step, &region);
        }
        if let (Some(popup), Some((id, view))) = (self.input.popup, self.popup_view()) {
            if let Ok(select) = self.select(id) {
                select.paint_popup(p, cx, &view, popup.highlight);
            }
        }
        self.paint_menu(p, cx);
        for (index, view) in toasts {
            if let Some(shown) = self.layers.toasts.get(*index) {
                shown.toast.paint(p, cx, view);
            }
        }
        if let Some(tip) = self.layers.tip {
            if let Some(label) = self.nodes[tip.node.index()].tooltip.as_ref() {
                paint_tooltip(p, cx, tip.rect, label);
            }
        }
    }

    fn paint_step(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, step: PaintStep, region: &Rect) {
        match step {
            PaintStep::Node(id) => self.paint_node(p, cx, id, region),
            PaintStep::ScrollBar(id) => self.paint_scrollbar(p, id, region),
        }
    }
}

/// Grows a logical rectangle so that its edges fall on device pixels.
pub(crate) fn snap_out(rect: Rect, scale: f32) -> Rect {
    let x0 = (rect.x * scale).floor() / scale;
    let y0 = (rect.y * scale).floor() / scale;
    let x1 = (rect.right() * scale).ceil() / scale;
    let y1 = (rect.bottom() * scale).ceil() / scale;
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
