//! Button (primary, plain, quiet, danger) and icon button.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::color::Color;
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::TextAlign;
use crate::tokens::{size, ColorRole, InputMode, Radius, Space};

/// The four button kinds of the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ButtonVariant {
    /// The one main action of a surface (brand gold).
    Primary,
    /// A normal action.
    #[default]
    Plain,
    /// A low-emphasis action without border.
    Quiet,
    /// A destructive action.
    Danger,
    /// A closable tab of a document strip (browser tabs): a 1-point transparent edge at the
    /// start (the chip's border), padding 12, the
    /// label semibold when `true` (the tab shown), regular otherwise; announced as a tab.
    Tab(bool),
}

/// A push button with a text label and an optional leading icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// The label (also the accessible name).
    pub label: Label,
    /// The kind.
    pub variant: ButtonVariant,
    /// Icon before the label.
    pub icon: Option<IconId>,
    /// A hold button reports `Pressed`, `Repeated` while held and `Released` instead of
    /// `Activated` (show a password while held, delete digits while held).
    pub hold: bool,
    /// Icon above the label in a tile (boards PhoneInCall controls) instead of a leading icon.
    pub tile: bool,
    /// A toggle: `Some(on)`; on = `hover` fill with a two-border `accent_text` edge, announced
    /// as pressed. `None` for an ordinary button.
    pub toggled: Option<bool>,
}

/// Inner spacing of a tile (inline, block).
const TILE_PAD: (Space, Space) = (Space::S4, Space::S8);
/// Space between the icon and the label of a tile.
const TILE_GAP: Space = Space::S4;

const LEAD: f32 = size::ICON_SMALL;
const LEAD_GAP: Space = Space::S8;
const PAD: Space = Space::S16;

impl Button {
    /// A plain button.
    pub fn new(label: Label) -> Button {
        Button {
            label,
            variant: ButtonVariant::Plain,
            icon: None,
            hold: false,
            tile: false,
            toggled: None,
        }
    }

    /// The same button as a tile: icon (24) above the label (small text, centred, wrapping).
    pub fn tile(mut self) -> Button {
        self.tile = true;
        self
    }

    /// The same button as a toggle in state `on` (announced as pressed when on).
    pub fn toggled(mut self, on: bool) -> Button {
        self.toggled = Some(on);
        self
    }

    /// The same button as a hold button.
    pub fn hold(mut self) -> Button {
        self.hold = true;
        self
    }

    /// The same button of another kind.
    pub fn variant(mut self, variant: ButtonVariant) -> Button {
        self.variant = variant;
        self
    }

    /// The same button with a leading icon.
    pub fn icon(mut self, icon: IconId) -> Button {
        self.icon = Some(icon);
        self
    }

    fn lead_width(&self, busy: bool) -> f32 {
        if self.icon.is_some() || busy {
            LEAD + LEAD_GAP.points()
        } else {
            0.0
        }
    }

    /// Inner padding at start and end: 12 for a tab, 16 for every other kind (boards).
    fn pad(&self) -> Space {
        match self.variant {
            ButtonVariant::Tab(_) => Space::S12,
            _ => PAD,
        }
    }

    /// Text of the label: a tab is semibold when shown and regular otherwise.
    fn text_spec(&self) -> crate::paint::TextSpec {
        match self.variant {
            ButtonVariant::Tab(true) => style::CONTROL_TEXT.weight(crate::text::Weight::Semibold),
            ButtonVariant::Tab(false) => style::CONTROL_TEXT.weight(crate::text::Weight::Regular),
            _ => style::BUTTON_TEXT,
        }
    }

    /// Width of the edge: two borders for the primary button, one for every other kind
    /// (the quiet button has a transparent one, as on the boards).
    /// With an error every kind has a two-border `danger` edge (boards).
    fn edge(&self, error: bool) -> f32 {
        let wide =
            self.toggled == Some(true) || (self.tile && self.variant == ButtonVariant::Danger);
        match (self.variant, error) {
            (ButtonVariant::Primary, _) | (_, true) => 2.0 * size::BORDER,
            _ if wide => 2.0 * size::BORDER,
            _ => size::BORDER,
        }
    }

    /// Size of the button; `(loading, error)` add the leading progress ring or error icon.
    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        (loading, error): (bool, bool),
    ) -> Size {
        let target = cx.mode.min_target();
        if self.tile {
            return self.measure_tile(cx, avail);
        }
        let text = cx.text_size(&self.label, self.text_spec(), None);
        // Boards: padding 16 inside the border (CSS border-box), so the edge adds its width.
        let lead = self.lead_width(loading || error);
        let edges = match self.variant {
            // A tab: the 1-point edge of its chip at the start only (the close button follows).
            ButtonVariant::Tab(_) if !error => size::BORDER,
            _ => 2.0 * self.edge(error),
        };
        let natural = text.w + lead + 2.0 * self.pad().points() + edges;
        Size::new(natural.min(avail).max(target), target)
    }

    /// Background, foreground and border colour for the state.
    fn colors(&self, p: &Painter<'_>, v: &Visual) -> (Option<Color>, Color, Option<Color>) {
        let neutral = p.color(style::neutral_background(v));
        if self.toggled == Some(true) && self.variant != ButtonVariant::Danger {
            let fill = if v.hovered || v.pressed {
                neutral
            } else {
                p.color(ColorRole::Hover)
            };
            let text = p.color(ColorRole::Text);
            return (Some(fill), text, Some(p.color(ColorRole::AccentText)));
        }
        match self.variant {
            ButtonVariant::Primary => {
                let fg = p.color(ColorRole::OnAccent);
                // The gold fill alone has 2:1 on light backgrounds: an accent-text edge
                // keeps the outline at 3:1 (boards: an `accent_text` edge).
                (
                    Some(style::filled(p.color(ColorRole::Accent), fg, v)),
                    fg,
                    Some(p.color(ColorRole::AccentText)),
                )
            }
            ButtonVariant::Plain => (
                Some(neutral),
                p.color(ColorRole::Text),
                Some(p.color(ColorRole::ControlBorder)),
            ),
            ButtonVariant::Quiet | ButtonVariant::Tab(_) => {
                let bg = (v.hovered || v.pressed).then_some(neutral);
                (bg, p.color(ColorRole::Text), None)
            }
            ButtonVariant::Danger => (
                Some(neutral),
                p.color(ColorRole::Danger),
                Some(p.color(ColorRole::Danger)),
            ),
        }
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        if self.tile {
            return self.paint_tile(p, cx, rect, v);
        }
        let previous = p.set_opacity(v.opacity());
        let (bg, fg, border) = self.colors(p, v);
        if let Some(bg) = bg {
            p.fill(rect, Some(Radius::Control), bg);
        }
        // Boards: an error gives every kind (also the quiet one) a `danger` edge.
        let border = if v.error {
            Some(p.color(ColorRole::Danger))
        } else {
            border
        };
        if let Some(border) = border {
            // The edge of the primary button is two borders wide: it alone carries the 3:1
            // the gold fill does not reach on light backgrounds.
            p.stroke(rect, Some(Radius::Control), self.edge(v.error), border);
        }
        let direction = p.env().direction;
        let lead = self.lead_width(v.loading || v.error);
        let edged = match self.variant {
            // The tab's edge is at its start only (see `measure`): its label has the rest.
            ButtonVariant::Tab(_) if !v.error => rect,
            _ => rect.inflate(-self.edge(v.error)),
        };
        let inner = style::inset_inline(edged, self.pad());
        let text = self.label.resolve(cx.tr);
        let layout = p.layout(&text, self.text_spec(), Some((inner.w - lead).max(0.0)));
        let text_width = layout.width / p.env().scale;
        let mut content =
            style::centered(inner, Size::new((text_width + lead).min(inner.w), inner.h));
        if lead > 0.0 {
            let mut slot = style::take_start(&mut content, lead, direction);
            let glyph = style::take_start(&mut slot, LEAD, direction);
            match (v.loading, self.icon) {
                (true, _) => p.spinner(style::centered(glyph, Size::new(LEAD, LEAD)), cx.spin, fg),
                (false, _) if v.error => p.icon(IconId::Error, glyph, LEAD, QuarterTurns::None, fg),
                (false, Some(icon)) => p.icon(icon, glyph, LEAD, QuarterTurns::None, fg),
                (false, None) => {}
            }
        }
        p.text(&layout, content, TextAlign::Center, fg);
        if v.focused {
            style::focus_ring(p, rect, Radius::Card);
        }
        p.set_opacity(previous);
    }
}

impl Button {
    /// Size of a tile: the wider of icon and label plus padding, icon + gap + label high.
    fn measure_tile(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let edge = self.edge(false);
        let inner_w = (avail - 2.0 * (TILE_PAD.0.points() + edge)).max(size::ICON);
        let label = cx.text_size(&self.label, style::SMALL_TEXT, Some(inner_w));
        let content_w = label.w.max(size::ICON);
        let w = content_w + 2.0 * (TILE_PAD.0.points() + edge);
        // A row of tiles is as high as its tile with the two-border edge (toggle on, danger):
        // every tile reserves that edge so that the row stays even (boards PhoneInCall).
        let wide = 2.0 * size::BORDER;
        let h = size::ICON + TILE_GAP.points() + label.h + 2.0 * (TILE_PAD.1.points() + wide);
        let target = cx.mode.min_target();
        Size::new(w.min(avail).max(target), h.max(target))
    }

    /// A tile: fill and edge as the button kind (toggle on: `hover` + `accent_text` edge),
    /// the icon centred at the top, the label centred below it.
    fn paint_tile(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        let (bg, fg, border) = self.colors(p, v);
        if let Some(bg) = bg {
            p.fill(rect, Some(Radius::Control), bg);
        }
        let border = if v.error {
            Some(p.color(ColorRole::Danger))
        } else {
            border
        };
        if let Some(border) = border {
            p.stroke(rect, Some(Radius::Control), self.edge(v.error), border);
        }
        let edge = self.edge(v.error);
        let inner = Rect::new(
            rect.x + TILE_PAD.0.points() + edge,
            rect.y + TILE_PAD.1.points() + edge,
            (rect.w - 2.0 * (TILE_PAD.0.points() + edge)).max(0.0),
            (rect.h - 2.0 * (TILE_PAD.1.points() + edge)).max(0.0),
        );
        let icon = match (v.loading, v.error, self.icon) {
            (_, true, _) => Some(IconId::Error),
            (false, false, icon) => icon,
            (true, false, _) => None,
        };
        let slot = Rect::new(
            inner.x + (inner.w - size::ICON) / 2.0,
            inner.y,
            size::ICON,
            size::ICON,
        );
        match icon {
            Some(icon) => p.icon(icon, slot, size::ICON, QuarterTurns::None, fg),
            None if v.loading => p.spinner(slot, cx.spin, fg),
            None => {}
        }
        let top = inner.y + size::ICON + TILE_GAP.points();
        let area = Rect::new(inner.x, top, inner.w, (inner.bottom() - top).max(0.0));
        style::draw_label(
            p,
            cx,
            &self.label,
            style::SMALL_TEXT,
            area,
            TextAlign::Center,
            fg,
        );
        if v.focused {
            style::focus_ring(p, rect, Radius::Card);
        }
        p.set_opacity(previous);
    }
}

/// A square button showing one icon; `name` is its accessible name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconButton {
    /// The icon.
    pub icon: IconId,
    /// Accessible name (catalog key).
    pub name: Label,
    /// A status button (the browser's tracking-protection button): a second icon after
    /// the first and the colour of both, padding 8 and 4 between the icons.
    pub status: Option<(IconId, ColorRole)>,
}

impl IconButton {
    /// An icon button.
    pub fn new(icon: IconId, name: Label) -> IconButton {
        IconButton {
            icon,
            name,
            status: None,
        }
    }

    /// The same button as a status button: `second` after the icon, both in `color`.
    pub fn status(mut self, second: IconId, color: ColorRole) -> IconButton {
        self.status = Some((second, color));
        self
    }

    /// Icon edge for an input mode: the small icon with a pointer, the large one for touch.
    pub(crate) fn icon_points(mode: InputMode) -> f32 {
        match mode {
            InputMode::Pointer => size::ICON_SMALL,
            InputMode::Touch => size::ICON,
        }
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>) -> Size {
        let target = cx.mode.min_target();
        if self.status.is_some() {
            let edge = IconButton::icon_points(cx.mode);
            let width = 2.0 * (Space::S8.points() + edge) + Space::S4.points();
            return Size::new(width.max(target), target);
        }
        Size::new(target, target)
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        if v.hovered || v.pressed {
            let bg = p.color(style::neutral_background(v));
            p.fill(rect, Some(Radius::Control), bg);
        }
        if v.error {
            // Boards: a two-border `danger` edge.
            let danger = p.color(ColorRole::Danger);
            p.stroke(rect, Some(Radius::Control), 2.0 * p.hairline(), danger);
        }
        let edge = IconButton::icon_points(cx.mode);
        if let Some((second, role)) = self.status {
            let fg = p.color(role);
            let inner = style::inset_inline(rect, Space::S8);
            let first = style::take_start(&mut inner.clone(), edge, p.env().direction);
            let mut rest = inner;
            let _ = style::take_start(&mut rest, edge + Space::S4.points(), p.env().direction);
            let then = style::take_start(&mut rest, edge, p.env().direction);
            p.icon(self.icon, first, edge, QuarterTurns::None, fg);
            p.icon(second, then, edge, QuarterTurns::None, fg);
            if v.focused {
                style::focus_ring(p, rect, Radius::Card);
            }
            p.set_opacity(previous);
            return;
        }
        let fg = p.color(ColorRole::Text);
        if v.loading {
            p.spinner(style::centered(rect, Size::new(edge, edge)), cx.spin, fg);
        } else {
            p.icon(self.icon, rect, edge, QuarterTurns::None, fg);
        }
        if v.focused {
            style::focus_ring(p, rect, Radius::Card);
        }
        p.set_opacity(previous);
    }
}
