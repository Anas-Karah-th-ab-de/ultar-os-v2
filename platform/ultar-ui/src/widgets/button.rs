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

    fn lead_width(&self, loading: bool) -> f32 {
        if self.icon.is_some() || loading {
            LEAD + LEAD_GAP.points()
        } else {
            0.0
        }
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32, loading: bool) -> Size {
        let target = cx.mode.min_target();
        let text = cx.text_size(&self.label, style::BUTTON_TEXT, None);
        let natural = text.w + self.lead_width(loading) + 2.0 * PAD.points();
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
            ButtonVariant::Quiet => {
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
        if let Some(border) = border {
            // The edge of the primary button is two borders wide: it alone carries the 3:1
            // the gold fill does not reach on light backgrounds.
            let primary = self.variant == ButtonVariant::Primary;
            let width = if primary {
                2.0 * p.hairline()
            } else {
                p.hairline()
            };
            p.stroke(rect, Some(Radius::Control), width, border);
        }
        let direction = p.env().direction;
        let lead = self.lead_width(v.loading);
        let inner = style::inset_inline(rect, PAD);
        let text = self.label.resolve(cx.tr);
        let layout = p.layout(&text, style::BUTTON_TEXT, Some((inner.w - lead).max(0.0)));
        let text_width = layout.width / p.env().scale;
        let mut content =
            style::centered(inner, Size::new((text_width + lead).min(inner.w), inner.h));
        if lead > 0.0 {
            let mut slot = style::take_start(&mut content, lead, direction);
            let glyph = style::take_start(&mut slot, LEAD, direction);
            match (v.loading, self.icon) {
                (true, _) => p.spinner(style::centered(glyph, Size::new(LEAD, LEAD)), cx.spin, fg),
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
}

impl IconButton {
    /// An icon button.
    pub fn new(icon: IconId, name: Label) -> IconButton {
        IconButton { icon, name }
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
        Size::new(target, target)
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        if v.hovered || v.pressed {
            let bg = p.color(style::neutral_background(v));
            p.fill(rect, Some(Radius::Control), bg);
        }
        let fg = p.color(ColorRole::Text);
        let edge = IconButton::icon_points(cx.mode);
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
