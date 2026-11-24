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
}

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
        }
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
        match (self.variant, error) {
            (ButtonVariant::Primary, _) | (_, true) => 2.0 * size::BORDER,
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
