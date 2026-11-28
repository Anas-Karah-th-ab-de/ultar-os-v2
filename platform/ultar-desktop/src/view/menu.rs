//! Menus of the bar (input language, power, the menu of an app tile) and of the desktop
//! background: a translucent popup with the toolkit's menu (board Menus) opened at its
//! corner.

use super::geometry::{self, Placement};
use super::{new_ui, Look, Target, View};
use ultar_i18n::Translator;
use ultar_ui::geom::{Point, Rect, Size};
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::ui::UiError;
use ultar_ui::widgets::Menu;

/// The menu's own size (laid out on a large surface).
fn card_size(menu: &Menu, look: Look, screen: Size, tr: &Translator) -> Result<Size, UiError> {
    let mut probe = new_ui(look, screen, tr)?;
    let root = probe.root();
    let owner = probe.add_sized(root, Flex::row(), Sizing::grow(1))?;
    probe.open_context_menu(owner, Point::new(0.0, 0.0), menu.clone())?;
    let rect = probe.menu_rect(0).ok_or(UiError::InvalidMenu)?;
    Ok(Size::new(rect.w, rect.h))
}

/// Builds a menu surface above the bar: `anchor_x` is where the button is (screen points
/// from the left); the menu opens above it.
pub fn build(
    menu: Menu,
    look: Look,
    screen: Size,
    anchor_x: f32,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    let card = card_size(&menu, look, screen, tr)?;
    let x = (anchor_x - card.w / 2.0).clamp(0.0, (screen.w - card.w).max(0.0));
    let y = screen.h - geometry::SHEET_BOTTOM - card.h;
    surface(menu, look, screen, Rect::new(x, y, card.w, card.h), tr)
}

/// Builds a menu surface at a screen point (the right click on the desktop background):
/// the menu opens beside the pointer towards the end side and below it, and turns to the
/// other side or above it where the screen (above the bar) has no room.
pub fn build_at(
    menu: Menu,
    look: Look,
    screen: Size,
    at: Point,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    let card = card_size(&menu, look, screen, tr)?;
    let rtl = look.lang.direction().is_rtl();
    let floor = screen.h - ultar_ui::tokens::size::BAR_HEIGHT;
    let before = at.x - card.w; // physical: left of the pointer
    let after = at.x;
    let x = match (rtl, before >= 0.0, after + card.w <= screen.w) {
        (true, true, _) | (false, true, false) => before,
        _ => after,
    };
    let x = x.clamp(0.0, (screen.w - card.w).max(0.0));
    let y = if at.y + card.h <= floor {
        at.y
    } else {
        at.y - card.h
    };
    let y = y.clamp(0.0, (floor - card.h).max(0.0));
    surface(menu, look, screen, Rect::new(x, y, card.w, card.h), tr)
}

/// The translucent surface around the menu card at `rect` (screen points), with the menu
/// opened at the card's corner.
fn surface(
    menu: Menu,
    look: Look,
    screen: Size,
    rect: Rect,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    let rtl = look.lang.direction().is_rtl();
    let placement = geometry::place(screen, rect, rtl);
    let mut ui = new_ui(look, placement.surface, tr)?;
    ui.set_transparent_background();
    let root = ui.root();
    let owner = ui.add_sized(root, Flex::row(), Sizing::grow(1))?;
    let mut view = View::new(ui);
    view.mark(owner, Target::Menu);
    let left = rect.x - placement.offset.0 as f32; // physical
    let top = rect.y - (placement.offset.1 as f32 + screen.h - ultar_ui::tokens::size::BAR_HEIGHT);
    let at = if rtl {
        Point::new(left + rect.w, top) // physical
    } else {
        Point::new(left, top) // physical
    };
    view.ui.open_context_menu(owner, at, menu)?;
    Ok((view, placement))
}
