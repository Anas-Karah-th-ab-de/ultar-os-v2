//! List and list item: rows the user picks from (navigation, files, results).

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::layout::Flex;
use crate::paint::{Painter, TextSpec};
use crate::text::{Overflow, TextAlign, Weight};
use crate::tokens::{size, state, typography, ColorRole, Radius, Space};

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
}

impl List {
    /// A list whose items only emit `Activated`.
    pub fn new() -> List {
        List {
            flex: Flex::column(),
            name: None,
            selectable: false,
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

    fn lead(&self) -> f32 {
        if self.icon.is_some() {
            size::ICON + GAP.points()
        } else {
            0.0
        }
    }

    fn text_width(&self, width: f32, control: f32) -> f32 {
        let taken = if control > 0.0 {
            control + GAP.points()
        } else {
            0.0
        };
        (width - 2.0 * PAD_INLINE.points() - self.lead() - taken).max(0.0)
    }

    fn label_spec(&self) -> TextSpec {
        ITEM_LABEL.weight(if self.selected {
            Weight::Semibold
        } else {
            Weight::Medium
        })
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32, control: Size) -> Size {
        let width = self.text_width(avail, control.w);
        let mut text = cx.text_size(&self.label, self.label_spec(), Some(width)).h;
        if let Some(sub) = &self.sublabel {
            text += cx.text_size(sub, style::SMALL_TEXT, Some(width)).h;
        }
        let content = text.max(control.h) + 2.0 * PAD_BLOCK.points();
        Size::new(avail, content.max(MIN_HEIGHT).max(cx.mode.min_target()))
    }

    /// Logical placement of the trailing control inside an item of `size`.
    pub(crate) fn control_offset(size: Size, control: Size) -> (f32, f32) {
        (
            size.w - PAD_INLINE.points() - control.w,
            (size.h - control.h) / 2.0,
        )
    }

    fn paint_texts(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, inner: Rect) {
        let scale = p.env().scale;
        let text = self.label.resolve(cx.tr);
        let label = p.layout(&text, self.label_spec(), Some(inner.w));
        let label_h = label.height / scale;
        let sub = self.sublabel.as_ref().map(|s| {
            let text = s.resolve(cx.tr);
            p.layout(&text, style::SMALL_TEXT, Some(inner.w))
        });
        let sub_h = sub.as_ref().map_or(0.0, |l| l.height / scale);
        let top = inner.y + (inner.h - label_h - sub_h) / 2.0;
        let color = p.color(ColorRole::Text);
        let area = Rect::new(inner.x, top, inner.w, label_h);
        p.text(&label, area, TextAlign::Start, color);
        if let Some(sub) = sub {
            let muted = p.color(ColorRole::TextMuted);
            let area = Rect::new(inner.x, top + label_h, inner.w, sub_h);
            p.text(&sub, area, TextAlign::Start, muted);
        }
    }

    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        control: f32,
    ) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        if v.pressed || v.hovered || self.selected {
            let bg = p.color(style::neutral_background(v));
            p.fill(rect, None, bg);
        }
        if self.selected {
            // Selection is a bar at the start edge and a heavier label, not colour alone.
            let accent = p.color(ColorRole::Accent);
            let mut edge = rect.inflate(-PAD_BLOCK.points());
            edge.x = rect.x;
            edge.w = rect.w;
            let bar = style::take_start(&mut edge, MARK.points(), direction);
            p.fill(bar, Some(Radius::Round), accent);
        }
        let mut inner = style::inset_inline(rect, PAD_INLINE);
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
        self.paint_texts(p, cx, inner);
        if v.focused {
            // Inside the item: rows touch each other and a list may be clipped by a scroller.
            let ring = p.color(state::FOCUS_RING);
            p.stroke(rect, Some(Radius::Control), size::FOCUS_RING, ring);
        }
        p.set_opacity(previous);
        if self.separator {
            let (line, hairline) = (p.color(ColorRole::Line), p.hairline());
            let y = rect.bottom() - hairline;
            p.fill(Rect::new(rect.x, y, rect.w, hairline), None, line);
        }
    }
}
