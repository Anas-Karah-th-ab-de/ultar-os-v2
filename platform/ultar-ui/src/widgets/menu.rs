//! Menu and context menu: a floating list of commands, with submenus that open towards the
//! end edge (to the left in right-to-left layouts).

use super::style;
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::TextAlign;
use crate::tokens::{size, state, ColorRole, Radius, Space};
use ultar_i18n::Direction;

/// Most items one menu level may hold.
pub const MAX_MENU_ITEMS: usize = 16;
/// Deepest nesting of submenus (the top level counts).
pub const MAX_MENU_DEPTH: usize = 4;

/// One command of a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Identifier reported in `ActionKind::MenuChosen` (chosen by the application).
    pub id: u32,
    /// The label.
    pub label: Label,
    /// Icon before the label.
    pub icon: Option<IconId>,
    /// Keyboard shortcut shown at the end (display only).
    pub shortcut: Option<Label>,
    /// The command is not available now.
    pub disabled: bool,
    /// The command destroys something.
    pub danger: bool,
    /// For commands that are on or off: the current state (shown by a check mark).
    pub checked: Option<bool>,
    /// Draw a separator line above this item.
    pub separated: bool,
    /// Commands that open when this item is chosen.
    pub submenu: Option<Menu>,
}

impl MenuItem {
    /// A command.
    pub fn new(id: u32, label: Label) -> MenuItem {
        MenuItem {
            id,
            label,
            icon: None,
            shortcut: None,
            disabled: false,
            danger: false,
            checked: None,
            separated: false,
            submenu: None,
        }
    }

    /// The same item with an icon.
    pub fn icon(mut self, icon: IconId) -> MenuItem {
        self.icon = Some(icon);
        self
    }

    /// The same item showing a keyboard shortcut.
    pub fn shortcut(mut self, shortcut: Label) -> MenuItem {
        self.shortcut = Some(shortcut);
        self
    }

    /// The same item, unavailable.
    pub fn disabled(mut self) -> MenuItem {
        self.disabled = true;
        self
    }

    /// The same item marked as destructive.
    pub fn danger(mut self) -> MenuItem {
        self.danger = true;
        self
    }

    /// The same item as an on/off command in the given state.
    pub fn checked(mut self, checked: bool) -> MenuItem {
        self.checked = Some(checked);
        self
    }

    /// The same item with a separator line above it.
    pub fn separated(mut self) -> MenuItem {
        self.separated = true;
        self
    }

    /// The same item opening a submenu.
    pub fn submenu(mut self, submenu: Menu) -> MenuItem {
        self.submenu = Some(submenu);
        self
    }
}

/// A list of commands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Menu {
    /// The items, top to bottom.
    pub items: Vec<MenuItem>,
}

const PANEL_PAD: Space = Space::S4;
const ROW_PAD: Space = Space::S8;
const SLOT: f32 = size::ICON_SMALL;
const SLOT_GAP: Space = Space::S8;
const SHORTCUT_GAP: Space = Space::S24;
const SEPARATOR: Space = Space::S8;
/// Narrowest menu (boards "Menus": file and context menus 224 wide).
const MIN_W: f32 = 4.0 * Space::S48.points() + Space::S32.points();
/// Narrowest submenu (boards "Menus": 160 wide).
const SUB_MIN_W: f32 = 3.0 * Space::S48.points() + Space::S16.points();

/// Distance of the rows from the panel's outer edge: the border plus the padding inside it
/// (boards: CSS border-box, the panel padding inside a one-border edge).
fn inset() -> f32 {
    PANEL_PAD.points() + size::BORDER
}

/// Geometry of one open menu level.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MenuPanel {
    pub(crate) rect: Rect,
    /// Top of each item's row, relative to the panel's top.
    tops: Vec<f32>,
    row_height: f32,
    /// Height of all rows; more than the panel when the menu does not fit the surface.
    content: f32,
    /// How far the rows are moved up (a menu taller than the surface scrolls).
    pub(crate) scroll: f32,
}

impl MenuPanel {
    /// The rectangle of item `index`.
    pub(crate) fn row(&self, index: usize) -> Option<Rect> {
        let top = self.tops.get(index)?;
        let pad = inset();
        Some(Rect::new(
            self.rect.x + pad,
            self.rect.y + top - self.scroll,
            self.rect.w - 2.0 * pad,
            self.row_height,
        ))
    }

    /// The item under `pos`.
    pub(crate) fn item_at(&self, pos: Point) -> Option<usize> {
        if !self.rect.contains(pos) {
            return None;
        }
        (0..self.tops.len()).find(|i| self.row(*i).is_some_and(|r| r.contains(pos)))
    }

    /// Moves the rows by `delta` points (clamped to the content).
    pub(crate) fn scroll_by(&mut self, delta: f32) {
        let max = (self.content - self.rect.h).max(0.0);
        let target = self.scroll + delta;
        self.scroll = if target.is_finite() {
            target.clamp(0.0, max)
        } else {
            0.0
        };
    }

    /// Scrolls so that item `index` is completely inside the panel.
    pub(crate) fn reveal(&mut self, index: usize) {
        let (Some(row), pad) = (self.row(index), inset()) else {
            return;
        };
        let (top, bottom) = (self.rect.y + pad, self.rect.bottom() - pad);
        if row.y < top {
            self.scroll_by(row.y - top);
        } else if row.bottom() > bottom {
            self.scroll_by(row.bottom() - bottom);
        }
    }
}

/// `value` moved into `low..=high` (`low` wins when the range is empty).
fn fit(value: f32, low: f32, high: f32) -> f32 {
    value.min(high).max(low)
}

impl Menu {
    /// A menu of `items`.
    pub fn new(items: Vec<MenuItem>) -> Menu {
        Menu { items }
    }

    /// `true` when the menu and all its submenus respect [`MAX_MENU_ITEMS`] and
    /// [`MAX_MENU_DEPTH`] and no level is empty.
    pub fn within_limits(&self) -> bool {
        fn check(menu: &Menu, depth: usize) -> bool {
            depth <= MAX_MENU_DEPTH
                && !menu.items.is_empty()
                && menu.items.len() <= MAX_MENU_ITEMS
                && menu
                    .items
                    .iter()
                    .filter_map(|item| item.submenu.as_ref())
                    .all(|sub| check(sub, depth + 1))
        }
        check(self, 1)
    }

    /// The submenu reached by following `path` (item indices from the top level).
    pub(crate) fn level(&self, path: &[usize]) -> Option<&Menu> {
        let mut menu = self;
        for index in path {
            menu = menu.items.get(*index)?.submenu.as_ref()?;
        }
        Some(menu)
    }

    /// The next item from `from` in direction `forward` that can be highlighted; wraps.
    pub(crate) fn step(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let count = self.items.len();
        let mut index = match (from, forward) {
            (Some(i), true) => (i + 1) % count.max(1),
            (Some(i), false) => (i + count - 1) % count.max(1),
            (None, true) => 0,
            (None, false) => count.saturating_sub(1),
        };
        for _ in 0..count {
            if !self.items[index].disabled {
                return Some(index);
            }
            index = if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            };
        }
        None
    }

    fn has_slot(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.icon.is_some() || i.checked.is_some())
    }

    fn has_submenu(&self) -> bool {
        self.items.iter().any(|i| i.submenu.is_some())
    }

    /// Natural size of the panel and the top of each row.
    fn measure(&self, cx: &mut MeasureCx<'_>, surface: Size, min_w: f32) -> (Size, Vec<f32>) {
        let (mut labels, mut shortcuts) = (0.0f32, 0.0f32);
        for item in &self.items {
            labels = labels.max(cx.text_size(&item.label, style::CONTROL_TEXT, None).w);
            if let Some(shortcut) = &item.shortcut {
                shortcuts = shortcuts.max(cx.text_size(shortcut, style::SMALL_TEXT, None).w);
            }
        }
        let mut width = 2.0 * (inset() + ROW_PAD.points()) + labels;
        if self.has_slot() {
            width += SLOT + SLOT_GAP.points();
        }
        if shortcuts > 0.0 {
            width += SHORTCUT_GAP.points() + shortcuts;
        }
        if self.has_submenu() {
            width += SLOT_GAP.points() + SLOT;
        }
        let limit = (surface.w - 2.0 * ROW_PAD.points()).max(0.0);
        let mut tops = Vec::with_capacity(self.items.len());
        let mut y = inset();
        for (index, item) in self.items.iter().enumerate() {
            if item.separated && index > 0 {
                y += SEPARATOR.points();
            }
            tops.push(y);
            y += cx.mode.min_target();
        }
        let size = Size::new(fit(width, min_w.min(limit), limit), y + inset());
        (size, tops)
    }

    fn panel(
        &self,
        cx: &mut MeasureCx<'_>,
        (surface, min_w): (Size, f32),
        place: impl Fn(Size) -> Point,
    ) -> MenuPanel {
        let (content, tops) = self.measure(cx, surface, min_w);
        let edge = PANEL_PAD.points();
        // A menu taller than the surface shows what fits and scrolls.
        let row_height = cx.mode.min_target();
        let room = (surface.h - 2.0 * edge).max(row_height + 2.0 * edge);
        let size = Size::new(content.w, content.h.min(room));
        let origin = place(size);
        let x = fit(origin.x, edge, surface.w - size.w - edge);
        let y = fit(origin.y, edge, surface.h - size.h - edge);
        MenuPanel {
            rect: Rect::new(x, y, size.w, size.h),
            tops,
            row_height,
            content: content.h,
            scroll: 0.0,
        }
    }

    /// The panel as a drop-down of `anchor`: below it (above when only that fits), aligned
    /// with its start edge.
    pub(crate) fn panel_below(
        &self,
        cx: &mut MeasureCx<'_>,
        surface: Size,
        anchor: Rect,
    ) -> MenuPanel {
        let rtl = cx.env.direction.is_rtl();
        let gap = PANEL_PAD.points();
        self.panel(cx, (surface, MIN_W), |size| {
            let x = if rtl {
                anchor.right() - size.w
            } else {
                anchor.x
            };
            let below = anchor.bottom() + gap;
            let above = anchor.y - gap - size.h;
            let y = if below + size.h > surface.h && above >= 0.0 {
                above
            } else {
                below
            };
            Point::new(x, y)
        })
    }

    /// The panel as a context menu at `pos`: it grows towards the end edge and downwards,
    /// and the other way where there is no room.
    pub(crate) fn panel_at(&self, cx: &mut MeasureCx<'_>, surface: Size, pos: Point) -> MenuPanel {
        let rtl = cx.env.direction.is_rtl();
        self.panel(cx, (surface, MIN_W), |size| {
            let x = match rtl {
                true if pos.x - size.w >= 0.0 => pos.x - size.w,
                true => pos.x,
                false if pos.x + size.w <= surface.w => pos.x,
                false => pos.x - size.w,
            };
            let y = if pos.y + size.h > surface.h {
                pos.y - size.h
            } else {
                pos.y
            };
            Point::new(x, y)
        })
    }

    /// The panel as a submenu of the item at `row` in the panel `parent`: beside the parent
    /// on its end side (left in right-to-left layouts), on the start side when only that fits.
    pub(crate) fn panel_beside(
        &self,
        cx: &mut MeasureCx<'_>,
        surface: Size,
        parent: Rect,
        row: Rect,
    ) -> MenuPanel {
        let rtl = cx.env.direction.is_rtl();
        // Boards: a 4-point gap beside the parent; the first row level with the parent row.
        let gap = Space::S4.points();
        let lift = inset();
        self.panel(cx, (surface, SUB_MIN_W), |size| {
            let right = parent.right() + gap;
            let left = parent.x - gap - size.w;
            let x = match rtl {
                true if left >= 0.0 => left,
                true => right,
                false if right + size.w <= surface.w => right,
                false if left >= 0.0 => left,
                false => right,
            };
            Point::new(x, row.y - lift)
        })
    }

    fn paint_row(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        item: &MenuItem,
        row: Rect,
        direction: Direction,
    ) {
        let previous = p.set_opacity([1.0, state::DISABLED_OPACITY][usize::from(item.disabled)]);
        let color = p.color([ColorRole::Text, ColorRole::Danger][usize::from(item.danger)]);
        let mut inner = style::inset_inline(row, ROW_PAD);
        if self.has_slot() {
            let slot = style::take_start(&mut inner, SLOT, direction);
            style::take_start(&mut inner, SLOT_GAP.points(), direction);
            match (item.checked, item.icon) {
                (Some(true), _) => p.icon(IconId::Check, slot, SLOT, QuarterTurns::None, color),
                (None, Some(icon)) => p.icon(icon, slot, SLOT, QuarterTurns::None, color),
                _ => {}
            }
        }
        if self.has_submenu() {
            let slot = style::take_end(&mut inner, SLOT, direction);
            style::take_end(&mut inner, SLOT_GAP.points(), direction);
            if item.submenu.is_some() {
                let muted = p.color(ColorRole::TextMuted);
                p.icon(IconId::Forward, slot, SLOT, QuarterTurns::None, muted);
            }
        }
        if let Some(shortcut) = &item.shortcut {
            let muted = p.color(ColorRole::TextMuted);
            let spec = style::SMALL_TEXT.overflow(crate::text::Overflow::Clip);
            style::draw_label(p, cx, shortcut, spec, inner, TextAlign::End, muted);
        }
        let spec = style::CONTROL_TEXT;
        style::draw_label(p, cx, &item.label, spec, inner, TextAlign::Start, color);
        p.set_opacity(previous);
    }

    /// Paints one level; `highlight` is the item under the keyboard or pointer, `open` the
    /// item whose submenu is showing.
    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        panel: &MenuPanel,
        highlight: Option<usize>,
        open: Option<usize>,
    ) {
        p.shadow(panel.rect, Some(Radius::Control));
        let (bg, line, hairline) = (
            p.color(ColorRole::Raised),
            p.color(ColorRole::Line),
            p.hairline(),
        );
        p.fill(panel.rect, Some(Radius::Control), bg);
        p.stroke(panel.rect, Some(Radius::Control), hairline, line);
        let direction = p.env().direction;
        let saved = p.clip(panel.rect);
        for (index, item) in self.items.iter().enumerate() {
            let Some(row) = panel.row(index) else {
                continue;
            };
            if item.separated && index > 0 {
                let y = row.y - SEPARATOR.points() / 2.0;
                p.fill(Rect::new(row.x, y, row.w, hairline), None, line);
            }
            if highlight == Some(index) || open == Some(index) {
                let hover = p.color(ColorRole::Hover);
                p.fill(row, Some(Radius::Control), hover);
            }
            self.paint_row(p, cx, item, row, direction);
        }
        p.restore_clip(saved);
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
