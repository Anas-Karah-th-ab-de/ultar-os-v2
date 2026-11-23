//! List and list item: rows the user picks from (navigation, files, results).

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::layout::Flex;
use crate::paint::PaintEnv;
use crate::paint::{Painter, TextSpec};
use crate::text::{Overflow, TextAlign, Weight};
use crate::tokens::{size, state, typography, ColorRole, Radius, Space, TypeSize};

/// A list: a column of [`ListItem`]s. When `selectable`, activating an item selects it
/// (and only it) and the list emits `Selected(index)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    /// Layout of the items (a column).
    pub flex: Flex,
    /// Accessible name (catalog key).
    pub name: Option<Label>,
    /// `true` when exactly one item at a time is the selected one.
    pub selectable: bool,
    /// `true` for the navigation list of a sidebar: compact items (one line, the target
    /// height, rounded), announced as navigation.
    pub navigation: bool,
}

impl List {
    /// A list whose items only emit `Activated`.
    pub fn new() -> List {
        List {
            flex: Flex::column(),
            name: None,
            selectable: false,
            navigation: false,
        }
    }

    /// A sidebar: a selectable navigation list with compact items and a gap between them.
    pub fn navigation() -> List {
        List {
            flex: Flex::column().gap(Space::S4),
            selectable: true,
            navigation: true,
            ..List::new()
        }
    }

    /// The same list with single selection.
    pub fn selectable(mut self) -> List {
        self.selectable = true;
        self
    }

    /// The same list with an accessible name.
    pub fn name(mut self, name: Label) -> List {
        self.name = Some(name);
        self
    }
}

impl Default for List {
    fn default() -> List {
        List::new()
    }
}

/// A list item: optional leading icon, label, optional sublabel and at most one child, the
/// trailing control (a switch, a badge, an icon button).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// The label (also the accessible name).
    pub label: Label,
    /// Second line under the label.
    pub sublabel: Option<Label>,
    /// Icon before the label.
    pub icon: Option<IconId>,
    /// `true` for the selected item of its list.
    pub selected: bool,
    /// Draw a separator line under the item.
    pub separator: bool,
}

const PAD_INLINE: Space = Space::S16;
const PAD_BLOCK: Space = Space::S8;
const GAP: Space = Space::S12;
const MIN_HEIGHT: f32 = Space::S48.points();
/// Inline padding of a navigation item (boards: 0 / 12).
const NAV_PAD: Space = Space::S12;
/// The second line: one line, cut with an ellipsis.
const SUBLABEL: TextSpec = TextSpec::new(TypeSize::S12).overflow(Overflow::Ellipsis);
const MARK: Space = Space::S4;
const ITEM_LABEL: TextSpec = TextSpec::new(typography::BODY).overflow(Overflow::Ellipsis);

impl ListItem {
    /// An item with a label.
    pub fn new(label: Label) -> ListItem {
        ListItem {
            label,
            sublabel: None,
            icon: None,
            selected: false,
            separator: false,
        }
    }

    /// The same item with a second line.
    pub fn sublabel(mut self, sublabel: Label) -> ListItem {
        self.sublabel = Some(sublabel);
        self
    }

    /// The same item with a leading icon.
    pub fn icon(mut self, icon: IconId) -> ListItem {
        self.icon = Some(icon);
        self
    }

    /// The same item marked as the selected one.
    pub fn selected(mut self) -> ListItem {
        self.selected = true;
        self
    }

    /// The same item with a separator line under it.
    pub fn separator(mut self) -> ListItem {
        self.separator = true;
        self
    }

    fn label_spec(&self) -> TextSpec {
        ITEM_LABEL.weight(if self.selected {
            Weight::Semibold
        } else {
            Weight::Medium
        })
    }

    /// Size of an item. No text is shaped: label and sublabel are one line each (cut with
    /// an ellipsis), so the height follows from the type tokens alone and a list of
    /// thousands of items is laid out without shaping a single label.
    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        control: Size,
        (nav, has_error): (bool, bool),
    ) -> Size {
        let target = cx.mode.min_target();
        if nav {
            return Size::new(avail, target.max(control.h));
        }
        let mut text = line_box(cx.env, typography::BODY);
        if self.sublabel.is_some() || has_error {
            text += line_box(cx.env, TypeSize::S12);
        }
        let content = text.max(control.h) + 2.0 * PAD_BLOCK.points();
        Size::new(avail, content.max(MIN_HEIGHT).max(target))
    }

    /// Logical placement of the trailing control inside an item of `size`.
    pub(crate) fn control_offset(size: Size, control: Size) -> (f32, f32) {
        (
            size.w - PAD_INLINE.points() - control.w,
            (size.h - control.h) / 2.0,
        )
    }

    /// Label and second line; an error message takes the place of the sublabel.
    fn paint_texts(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        inner: Rect,
        error: Option<&Label>,
    ) {
        let scale = p.env().scale;
        let text = self.label.resolve(cx.tr);
        let label = p.layout(&text, self.label_spec(), Some(inner.w));
        let label_h = label.height / scale;
        let second = error.or(self.sublabel.as_ref());
        let sub = second.map(|s| {
            let text = s.resolve(cx.tr);
            p.layout(&text, SUBLABEL, Some(inner.w))
        });
        let sub_h = sub.as_ref().map_or(0.0, |l| l.height / scale);
        let top = inner.y + (inner.h - label_h - sub_h) / 2.0;
        let color = p.color(ColorRole::Text);
        let area = Rect::new(inner.x, top, inner.w, label_h);
        p.text(&label, area, TextAlign::Start, color);
        if let Some(sub) = sub {
            let muted = p.color(if error.is_some() {
                ColorRole::Danger
            } else {
                ColorRole::TextMuted
            });
            let area = Rect::new(inner.x, top + label_h, inner.w, sub_h);
            p.text(&sub, area, TextAlign::Start, muted);
        }
    }

    /// The state background and the selection mark.
    fn paint_background(&self, p: &mut Painter<'_>, rect: Rect, v: &Visual, nav: bool) {
        if v.pressed || v.hovered || self.selected {
            // Rounded and inside the border: the first and last item never cover the
            // rounded corner of the card around the list.
            let bg = p.color(style::neutral_background(v));
            let area = if nav {
                rect
            } else {
                rect.inflate(-p.hairline())
            };
            p.fill(area, Some(Radius::Control), bg);
        }
        if self.selected {
            // Selection is a bar at the start edge and a heavier label, not colour alone
            // (boards: `accent_text`, 4 × 16, centred).
            let accent = p.color(ColorRole::AccentText);
            let mut edge = rect;
            let bar = style::take_start(&mut edge, MARK.points(), p.env().direction);
            let bar = style::centered(bar, Size::new(MARK.points(), size::ICON_SMALL));
            p.fill(bar, Some(Radius::Round), accent);
        }
    }

    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        (control, nav, error): (f32, bool, Option<&Label>),
    ) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        self.paint_background(p, rect, v, nav);
        if v.loading {
            let mut end = style::inset_inline(rect, PAD_INLINE);
            let slot = style::take_end(&mut end, size::ICON_SMALL, direction);
            let ring = style::centered(slot, Size::new(size::ICON_SMALL, size::ICON_SMALL));
            let muted = p.color(ColorRole::TextMuted);
            p.spinner(ring, cx.spin, muted);
        }
        let pad = if nav { NAV_PAD } else { PAD_INLINE };
        let mut inner = style::inset_inline(rect, pad);
        let control = if v.loading {
            control.max(size::ICON_SMALL)
        } else {
            control
        };
        if control > 0.0 {
            style::take_end(&mut inner, control + GAP.points(), direction);
        }
        if let Some(icon) = self.icon {
            let slot = style::take_start(&mut inner, size::ICON, direction);
            style::take_start(&mut inner, GAP.points(), direction);
            let role = if self.selected {
                ColorRole::AccentText
            } else {
                ColorRole::TextMuted
            };
            let color = p.color(role);
            p.icon(icon, slot, size::ICON, QuarterTurns::None, color);
        }
        self.paint_texts(p, cx, inner, error);
        if v.focused {
            // Inside the item: rows touch each other and a list may be clipped by a scroller.
            let ring = p.color(state::FOCUS_RING);
            p.stroke(rect, Some(Radius::Control), size::FOCUS_RING, ring);
        }
        p.set_opacity(previous);
        if self.separator && !nav {
            let (line, hairline) = (p.color(ColorRole::Line), p.hairline());
            let y = rect.bottom() - hairline;
            p.fill(Rect::new(rect.x, y, rect.w, hairline), None, line);
        }
    }
}

/// Height of one line of `size` text in logical points, from the type tokens alone (the
/// language decides the line height).
pub(crate) fn line_box(env: &PaintEnv, size: TypeSize) -> f32 {
    let height = if env.lang == ultar_i18n::Lang::Ar {
        typography::LINE_HEIGHT_ARABIC
    } else {
        typography::LINE_HEIGHT_LATIN
    };
    (size.points() * env.scale * height).round().max(1.0) / env.scale
}
