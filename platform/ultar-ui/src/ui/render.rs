//! Painting the tree: only damaged regions are redrawn.

use super::{NodeId, Ui};
use crate::canvas::Canvas;
use crate::geom::Rect;
use crate::paint::Painter;
use crate::tokens::Space;
use crate::widgets::{focus_outset, shadow_reach, PaintCx, PopupView, Select, Widget};

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
            let reach = shadow_reach();
            let grown = view.rect.inflate(reach.x);
            self.damage(Rect::new(grown.x, grown.y, grown.w, grown.h + reach.y));
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
            widget if widget.is_interactive() => {
                rect.inflate(focus_outset().max(Space::S4.points()))
            }
            _ => rect,
        }
    }

    fn paint_node(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, id: NodeId, region: &Rect) {
        let node = &self.nodes[id.index()];
        let rect = node.frame.resolve(self.env.direction, self.size().w);
        if !self.ink_bounds(id, rect).intersects(region) {
            return;
        }
        let v = self.visual(id);
        let error = node.error.as_ref();
        match &node.widget {
            Widget::Flex(_) => {}
            Widget::Text(w) => w.paint(p, cx, rect),
            Widget::Icon(w) => w.paint(p, rect),
            Widget::Button(w) => w.paint(p, cx, rect, &v),
            Widget::IconButton(w) => w.paint(p, cx, rect, &v),
            Widget::Switch(w) => w.paint(p, cx, rect, &v, self.knob_position(w.on, w.toggled_at)),
            Widget::Select(w) => w.paint(p, cx, rect, &v, error),
            Widget::Slider(w) => w.paint(p, rect, &v),
            Widget::TextField(w) => {
                let has_focus = self.input.focus == Some(id) && !v.disabled;
                w.paint(p, cx, rect, &v, has_focus, error);
            }
            Widget::Row(w) => {
                let control = node
                    .children
                    .first()
                    .map_or(0.0, |c| self.nodes[c.index()].frame.width);
                w.paint(p, cx, rect, &v, control);
            }
            Widget::Card(w) => w.paint(p, rect),
        }
    }

    fn paint_region(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, region: Rect) {
        let saved = p.clip(region);
        let background = p.color(self.background);
        p.fill(region, None, background);
        for id in &self.order {
            self.paint_node(p, cx, *id, &region);
        }
        if let (Some(popup), Some((id, view))) = (self.input.popup, self.popup_view()) {
            if let Ok(select) = self.select(id) {
                select.paint_popup(p, cx, &view, popup.highlight);
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
