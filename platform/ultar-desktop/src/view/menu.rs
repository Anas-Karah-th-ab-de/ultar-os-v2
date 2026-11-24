//! Menus of the bar (input language, power, the menu of an app tile): a translucent popup
//! with the toolkit's menu (board Menus) opened at its corner.

use super::geometry::{self, Placement};
use super::{new_ui, Look, Target, View};
use ultar_i18n::Translator;
use ultar_ui::geom::{Point, Size};
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::ui::UiError;
use ultar_ui::widgets::Menu;

/// Builds a menu surface above the bar: `anchor_x` is where the button is (screen points
/// from the left); the menu opens above it.
pub fn build(
    menu: Menu,
    look: Look,
    screen: Size,
    anchor_x: f32,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    // First pass: the menu's size on a large surface.
    let mut probe = new_ui(look, screen, tr)?;
    let root = probe.root();
    let owner = probe.add_sized(root, Flex::row(), Sizing::grow(1))?;
    probe.open_context_menu(owner, Point::new(0.0, 0.0), menu.clone())?;
    let rect = probe.menu_rect(0).ok_or(UiError::InvalidMenu)?;
    let card = Size::new(rect.w, rect.h);
    let x = (anchor_x - card.w / 2.0).clamp(0.0, (screen.w - card.w).max(0.0));
    let y = screen.h - geometry::SHEET_BOTTOM - card.h;
    let rtl = look.lang.direction().is_rtl();
    let placement = geometry::place(screen, ultar_ui::geom::Rect::new(x, y, card.w, card.h), rtl);
    let mut ui = new_ui(look, placement.surface, tr)?;
    ui.set_transparent_background();
    let root = ui.root();
    let owner = ui.add_sized(root, Flex::row(), Sizing::grow(1))?;
    let mut view = View::new(ui);
    view.mark(owner, Target::Menu);
    let left = x - placement.offset.0 as f32; // physical
    let top = y - (placement.offset.1 as f32 + screen.h - ultar_ui::tokens::size::BAR_HEIGHT);
    let at = if rtl {
        Point::new(left + card.w, top) // physical
    } else {
        Point::new(left, top) // physical
    };
    view.ui.open_context_menu(owner, at, menu)?;
    Ok((view, placement))
}
