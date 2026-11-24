//! Where the bar's surfaces go (board geometry of `gen.py`): the bar is
//! `size.bar_height` high at the bottom; sheets open above its end, `g(16)` = 64 above the
//! screen's bottom edge and `space` 8 from the end edge (`sheet()`), with the shadow inside
//! their translucent surface.

use ultar_ui::geom::{Rect, Size};
use ultar_ui::tokens::{shadow, size, Space};

/// Distance of a sheet's bottom edge from the screen's bottom edge (`g(16)`).
pub const SHEET_BOTTOM: f32 = 16.0 * Space::S4.points();
/// Distance of a sheet from the end edge (`s2`).
pub const SHEET_END: Space = Space::S8;
/// Quick settings width (`g(112)`).
pub const QUICK_WIDTH: f32 = 112.0 * Space::S4.points();
/// Calendar sheet width (`g(184)`).
pub const CALENDAR_WIDTH: f32 = 184.0 * Space::S4.points();
/// All-apps sheet width (no board: the quick settings' width plus a column).
pub const APPS_WIDTH: f32 = 160.0 * Space::S4.points();
/// Width of the bar's search entry (`g(50)`).
pub const SEARCH_WIDTH: f32 = 50.0 * Space::S4.points();
/// Display mode panel (board Projection: 225 × 80 grid units, inset 24).
pub const PROJECTION_SIZE: Size = Size::new(
    225.0 * Space::S4.points() - 2.0 * Space::S24.points(),
    80.0 * Space::S4.points() - 2.0 * Space::S24.points(),
);

/// The space a floating card's shadow needs around it (top/sides: blur; below: blur and
/// offset). Margins are space-scale values so that the tree can lay them out.
pub const SHADOW_SIDE: Space = Space::S32;
/// Bottom margin of the shadow (`offset_y` + blur rounded up to the scale).
pub const SHADOW_BELOW: Space = Space::S48;

/// A translucent popup holding a floating card: where the card is on the screen, where the
/// surface is (relative to the bar's panel) and the margins inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The surface relative to the panel's top-left corner (logical points).
    pub offset: (i32, i32),
    /// Surface size.
    pub surface: Size,
    /// Margin before the card at the start edge (logical).
    pub start: Space,
    /// Margin above the card.
    pub top: Space,
}

/// The largest space value not above `points`.
fn space_at_most(points: f32) -> Option<Space> {
    Space::ALL.into_iter().rev().find(|s| s.points() <= points)
}

/// A card of `card` size whose end edge is [`SHEET_END`] from the screen's end edge and
/// whose bottom is [`SHEET_BOTTOM`] above the screen's bottom (sheets of the bar).
pub fn sheet(screen: Size, card: Size, rtl: bool) -> Placement {
    let end_gap = SHEET_END.points();
    let x = if rtl {
        end_gap
    } else {
        screen.w - end_gap - card.w
    };
    let y = screen.h - SHEET_BOTTOM - card.h;
    place(screen, Rect::new(x, y, card.w, card.h), rtl)
}

/// A card centred horizontally above the bar (all apps).
pub fn centred_sheet(screen: Size, card: Size, rtl: bool) -> Placement {
    let x = ((screen.w - card.w) / 2.0).max(0.0);
    let y = screen.h - SHEET_BOTTOM - card.h;
    place(screen, Rect::new(x, y, card.w, card.h), rtl)
}

/// A card in the middle of the screen (display modes).
pub fn centred(screen: Size, card: Size, rtl: bool) -> Placement {
    let x = ((screen.w - card.w) / 2.0).max(0.0);
    let y = ((screen.h - size::BAR_HEIGHT - card.h) / 2.0).max(0.0);
    place(screen, Rect::new(x, y, card.w, card.h), rtl)
}

/// The surface around `card` (screen coordinates): shadow margins, cut at the screen edges
/// to a space value.
pub fn place(screen: Size, card: Rect, rtl: bool) -> Placement {
    let side = SHADOW_SIDE.points();
    let left = space_at_most(card.x.min(side)).map_or(0.0, Space::points); // physical
    let right = space_at_most((screen.w - card.right()).min(side)).map_or(0.0, Space::points); // physical
    let top = space_at_most(card.y.min(side)).unwrap_or(Space::S4);
    let below = (screen.h - card.bottom()).clamp(0.0, SHADOW_BELOW.points());
    let x = card.x - left; // physical
    let y = card.y - top.points();
    let surface = Size::new(left + card.w + right, top.points() + card.h + below); // physical
    let start_points = if rtl { right } else { left }; // physical
    let panel_top = screen.h - size::BAR_HEIGHT;
    Placement {
        offset: (x.round() as i32, (y - panel_top).round() as i32),
        surface,
        start: space_at_most(start_points).unwrap_or(Space::S4),
        top,
    }
}

/// The overview: the whole work area above the bar.
pub fn work_area(screen: Size) -> (Size, (i32, i32)) {
    let h = (screen.h - size::BAR_HEIGHT).max(0.0);
    (Size::new(screen.w, h), (0, -(h.round() as i32)))
}

/// Shadow reach of the toolkit (for checks).
pub fn shadow_reach() -> (f32, f32) {
    (shadow::BLUR, shadow::BLUR + shadow::OFFSET_Y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_sit_where_the_boards_put_them() {
        let screen = Size::new(1440.0, 900.0);
        let card = Size::new(QUICK_WIDTH, 400.0);
        let ltr = sheet(screen, card, false);
        // Card right edge at 1432, bottom at 836; start margin 32, end margin 8.
        assert_eq!(ltr.start, Space::S32);
        assert_eq!(ltr.surface.w, 32.0 + 448.0 + 8.0);
        assert_eq!(ltr.offset.0 as f32 + 32.0 + 448.0, 1432.0);
        assert_eq!(ltr.offset.1, (436 - 32) - 844);
        let rtl = sheet(screen, card, true);
        assert_eq!(rtl.offset.0, 0, "8 from the left edge in Arabic"); // physical
        assert_eq!(rtl.start, Space::S32);
        let (wa, off) = work_area(screen);
        assert_eq!((wa.h, off.1), (844.0, -844));
        let mid = centred(screen, PROJECTION_SIZE, false);
        assert_eq!(PROJECTION_SIZE, Size::new(852.0, 272.0));
        assert!(mid.surface.w > PROJECTION_SIZE.w);
        assert!(shadow_reach().1 <= SHADOW_BELOW.points());
        assert_eq!(space_at_most(3.0), None);
        let tiny = place(
            Size::new(100.0, 100.0),
            Rect::new(0.0, 0.0, 100.0, 90.0),
            false,
        );
        assert_eq!(tiny.top, Space::S4);
        let apps = centred_sheet(screen, Size::new(APPS_WIDTH, 300.0), false);
        assert_eq!(apps.surface.w, 32.0 + APPS_WIDTH + 32.0);
    }
}
