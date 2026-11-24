//! List and list item: rows the user picks from (navigation, files, results).
//!
//! Every item is a node (with its own trailing control, tooltip and context menu), so a
//! list is meant for up to a few hundred rows; `MAX_NODES` bounds a tree. Long data –
//! a folder of thousands of files, a log, search results – belongs in a [`super::Table`]:
//! its rows are data, one node for the whole table, only the visible rows are painted, and
//! `Ui::set_table_rows` replaces them in place.

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
    /// `true` when several items can be selected (Ctrl toggles, Shift extends).
    pub multi: bool,
    /// Where a Shift range starts (index among the items).
    pub(crate) anchor: Option<usize>,
}

impl List {
    /// A list whose items only emit `Activated`.
    pub fn new() -> List {
        List {
            flex: Flex::column(),
            name: None,
            selectable: false,
            navigation: false,
            multi: false,
            anchor: None,
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

    /// The same list with multi-selection: a click selects one item, Ctrl with a click or
    /// Space toggles an item, Shift with a click or an arrow selects a range.
    pub fn multi(mut self) -> List {
        self.selectable = true;
        self.multi = true;
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
    /// Show a forward arrow at the end: activating the item opens a page of its own.
    pub chevron: bool,
    /// Draw the item as a tile of a grid (a file or a desktop icon): the icon in a box above
    /// the label ([`ListItem::tile`]).
    pub tile: bool,
    /// A picture shown in the tile's box instead of the icon.
    pub thumbnail: Option<super::Thumbnail>,
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
            chevron: false,
            tile: false,
            thumbnail: None,
        }
    }

    /// The same item with a forward arrow at the end (it opens a page of its own; boards:
    /// `forward`, 16, mirrored in right-to-left layouts).
    pub fn chevron(mut self) -> ListItem {
        self.chevron = true;
        self
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
        if self.tile {
            return self.measure_tile(cx);
        }
        let target = cx.mode.min_target();
        let two_lines = self.sublabel.is_some() || has_error;
        if nav {
            // One line: a target high (sidebar). Two lines: a full row (boards: 56).
            let row = if two_lines { size::ROW_MIN } else { 0.0 };
            return Size::new(avail, target.max(control.h).max(row));
        }
        let mut text = line_box(cx.env, typography::BODY);
        if two_lines {
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
            // A sidebar marks its current item at the end edge (approved boards), other
            // lists at the start edge.
            let accent = p.color(ColorRole::AccentText);
            let mut edge = if nav {
                style::inset_inline(rect, NAV_PAD)
            } else {
                rect
            };
            let direction = p.env().direction;
            let bar = if nav {
                style::take_end(&mut edge, MARK.points(), direction)
            } else {
                style::take_start(&mut edge, MARK.points(), direction)
            };
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
        if self.tile {
            return self.paint_tile(p, cx, rect, v);
        }
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        self.paint_background(p, rect, v, nav);
        let pad = if nav { NAV_PAD } else { PAD_INLINE };
        let mut inner = style::inset_inline(rect, pad);
        if nav && !self.chevron {
            // Room for the selection mark at the end edge.
            style::take_end(&mut inner, MARK.points() + GAP.points(), direction);
        }
        self.paint_end_mark(p, cx, &mut inner, v, error.is_some());
        let control = if v.loading && !self.chevron {
            control.max(size::ICON_SMALL)
        } else {
            control
        };
        if control > 0.0 {
            style::take_end(&mut inner, control + GAP.points(), direction);
        }
        if let Some(icon) = self.icon {
            // A sidebar or page list shows the small icon (boards: 16), other lists 24.
            let edge = if nav { size::ICON_SMALL } else { size::ICON };
            let slot = style::take_start(&mut inner, edge, direction);
            style::take_start(&mut inner, GAP.points(), direction);
            let role = if self.selected {
                ColorRole::AccentText
            } else {
                ColorRole::TextMuted
            };
            let color = p.color(role);
            p.icon(icon, slot, edge, QuarterTurns::None, color);
        }
        self.paint_texts(p, cx, inner, error);
        if v.focused {
            if nav {
                // Boards: the ring 2 points outside the item (the gap between items).
                style::focus_ring(p, rect, Radius::Card);
            } else {
                // Inside the item: rows touch each other and a scroller may clip the list.
                let ring = p.color(state::FOCUS_RING);
                p.stroke(rect, Some(Radius::Control), size::FOCUS_RING, ring);
            }
        }
        p.set_opacity(previous);
    }

    /// The forward arrow of an item that opens a page, or the progress ring of a loading
    /// item, at the end of `inner` (which loses that slot).
    fn paint_end_mark(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        inner: &mut Rect,
        v: &Visual,
        failed: bool,
    ) {
        let direction = p.env().direction;
        if self.chevron {
            let slot = style::take_end(inner, size::ICON_SMALL, direction);
            style::take_end(inner, GAP.points(), direction);
            let color = p.color(ColorRole::Text);
            if failed {
                // Boards: the error icon in `danger` takes the arrow's place.
                let danger = p.color(ColorRole::Danger);
                p.icon(
                    IconId::Error,
                    slot,
                    size::ICON_SMALL,
                    QuarterTurns::None,
                    danger,
                );
            } else if v.loading {
                let ring = style::centered(slot, Size::new(size::ICON_SMALL, size::ICON_SMALL));
                p.spinner(ring, cx.spin, color);
            } else {
                let turns = QuarterTurns::None;
                p.icon(IconId::Forward, slot, size::ICON_SMALL, turns, color);
            }
        } else if v.loading {
            // The ring takes the place of the trailing control (room kept by the caller).
            let mut end = *inner;
            let slot = style::take_end(&mut end, size::ICON_SMALL, direction);
            let ring = style::centered(slot, Size::new(size::ICON_SMALL, size::ICON_SMALL));
            let muted = p.color(ColorRole::TextMuted);
            p.spinner(ring, cx.spin, muted);
        }
    }

    /// The separator under the item: part of the list, not of the item, so it keeps full
    /// strength under a disabled item.
    pub(crate) fn paint_separator(&self, p: &mut Painter<'_>, rect: Rect, nav: bool) {
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
