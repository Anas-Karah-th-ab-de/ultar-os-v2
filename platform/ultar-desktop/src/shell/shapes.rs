//! The shell's other shapes (DESKTOP-SPEC §5.5, §11): the phone (status bar, home screen,
//! gesture bar; sheets over the whole screen) and the desktop variant with a top bar and a
//! dock. The same state and the same clicks; only the panels and where popups go differ.

use super::render::{bar_text, menu_popup, Popup};
use super::Shell;
use crate::model::time;
use crate::model::{Open, Shape};
use crate::view::compact::{self, quick::Head, StatusText};
use crate::view::dock::{DOCK_PANEL, TOP_HEIGHT};
use crate::view::{self, geometry, Look, View};
use ultar_i18n::datefmt::ClockStyle;
use ultar_surface::proto::Edge;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::tokens::{size, Space};
use ultar_ui::ui::UiError;

/// Edge and thickness of the shell's first panel.
pub fn bar_panel(shape: Shape) -> (Edge, f32) {
    if shape.is_compact() {
        (Edge::Top, compact::STATUS_HEIGHT)
    } else if shape.is_top() {
        (Edge::Top, TOP_HEIGHT)
    } else {
        (Edge::Bottom, size::BAR_HEIGHT)
    }
}

/// Edge and thickness of the second panel, and whether its pixels have alpha: the phone's
/// gesture bar or the dock.
pub fn second_panel(shape: Shape) -> Option<(Edge, f32, bool)> {
    if shape.is_compact() {
        Some((Edge::Bottom, compact::GESTURE_HEIGHT, false))
    } else if shape.is_top() {
        Some((Edge::Bottom, DOCK_PANEL, true))
    } else {
        None
    }
}

/// The second panel's tree for a screen width.
pub fn build_second(s: &Shell, look: Look, width: f32) -> Result<Option<View>, UiError> {
    if s.shape.is_compact() {
        compact::gesture_bar(look, width, &s.tr).map(Some)
    } else if s.shape.is_top() {
        view::dock::dock(&s.d, look, width, &s.tr).map(Some)
    } else {
        Ok(None)
    }
}

/// The phone's home screen (the background surface).
pub fn build_home(s: &Shell, look: Look, screen: Size) -> Result<Option<View>, UiError> {
    if !s.shape.is_compact() {
        return Ok(None);
    }
    compact::home::build(&s.d, s.local_time(), look, screen, &s.tr).map(Some)
}

/// What the phone's status bar says: network, mobile network (with a modem), battery.
fn status_text(s: &Shell) -> StatusText {
    let p = &s.d.presence;
    let mut icons = vec![super::render::net_icon(s)];
    if p.modem == Some(true) {
        icons.push(IconId::Cell);
    }
    let battery = s.d.values.battery.map(|(percent, _)| percent);
    if battery.is_some() {
        icons.push(IconId::Battery);
    }
    StatusText {
        time: bar_text(s).time,
        icons,
        battery,
        open: matches!(s.d.open, Open::Quick(_)),
    }
}

pub(super) fn status_bar(s: &Shell, look: Look, width: f32) -> Result<View, UiError> {
    compact::status_bar(&status_text(s), look, width, &s.tr)
}

/// A popup of the phone: offsets are screen coordinates (the status bar is at the top).
pub(super) fn compact_popup(s: &Shell, look: Look, screen: Size) -> Result<Option<Popup>, UiError> {
    let below_status = (0, compact::STATUS_HEIGHT as i32);
    let full = |view: View, size: Size, offset: (i32, i32), translucent: bool| Popup {
        view,
        offset,
        size,
        translucent,
        inset_top: 0.0,
    };
    Ok(Some(match s.d.open {
        Open::Quick(_) | Open::Calendar => {
            let (time, date) = s
                .local_time()
                .map(|t| time::bar_lines(t, ClockStyle::H24, &s.tr))
                .unwrap_or_default();
            let now = (s.clock.now_ms(), i64::from(s.clock.offset_s()));
            let head = Head { time, date, now };
            let (v, size) = compact::quick::build(&s.d, &head, look, screen, &s.tr)?;
            full(v, size, (0, 0), true)
        }
        Open::Apps(category) => {
            let (v, size) = compact::apps::build(&s.d, category, look, screen, &s.tr)?;
            full(v, size, below_status, false)
        }
        Open::Overview => {
            let (v, size) = compact::search::build(&s.d, look, screen, &s.tr)?;
            full(v, size, below_status, false)
        }
        Open::KeyboardMenu
        | Open::PowerMenu
        | Open::OutputMenu
        | Open::DriveMenu(_)
        | Open::TileMenu(_)
        | Open::AppMenu => {
            let Some(mut popup) = menu_popup(s, look, screen, screen.w / 2.0)? else {
                return Ok(None);
            };
            popup.offset.1 += (screen.h - size::BAR_HEIGHT).round() as i32;
            popup
        }
        Open::None | Open::Projection(_) => return Ok(None),
    }))
}

/// Size of the overview and its place relative to the first panel: the work area.
pub(super) fn overview_area(shape: Shape, screen: Size) -> (Size, (i32, i32)) {
    if shape.is_top() {
        let h = (screen.h - TOP_HEIGHT - DOCK_PANEL).max(0.0);
        (Size::new(screen.w, h), (0, TOP_HEIGHT as i32))
    } else {
        geometry::work_area(screen)
    }
}

/// Where a menu of the bar opens (screen points from the left): the active app's menu at
/// the start of the top bar, the others in the middle.
pub(super) fn menu_anchor(s: &Shell, look: Look, screen: Size) -> f32 {
    let start = 30.0 * Space::S4.points();
    match (s.d.open, look.lang.direction().is_rtl()) {
        (Open::AppMenu, true) => screen.w - start,
        (Open::AppMenu, false) => start,
        _ => screen.w / 2.0,
    }
}

/// Moves a popup laid out for the bottom bar to where it belongs with the top bar (whose
/// panel is the popups' parent, at the top-left corner of the screen): sheets and the
/// bar's menus hang below the top bar, a dock tile's menu stands above the dock, the rest
/// keeps its place on the screen.
pub(super) fn for_top_bar(open: Open, popup: &mut Popup, screen: Size) {
    let bottom_panel = (screen.h - size::BAR_HEIGHT).round() as i32;
    let gap = Space::S8.points();
    match open {
        Open::Overview | Open::None => {}
        Open::Quick(_)
        | Open::Calendar
        | Open::KeyboardMenu
        | Open::PowerMenu
        | Open::OutputMenu
        | Open::AppMenu => {
            popup.offset.1 = (TOP_HEIGHT + gap - popup.inset_top).round() as i32;
        }
        Open::TileMenu(_) | Open::DriveMenu(_) => popup.offset.1 += bottom_panel - gap as i32,
        Open::Apps(_) | Open::Projection(_) => popup.offset.1 += bottom_panel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_follow_the_shape() {
        assert_eq!(bar_panel(Shape::default()), (Edge::Bottom, 56.0));
        assert_eq!(bar_panel(Shape::TOP), (Edge::Top, 32.0));
        assert_eq!(bar_panel(Shape::COMPACT), (Edge::Top, 32.0));
        assert_eq!(second_panel(Shape::default()), None);
        assert_eq!(second_panel(Shape::TOP), Some((Edge::Bottom, 68.0, true)));
        assert_eq!(
            second_panel(Shape::COMPACT),
            Some((Edge::Bottom, 44.0, false))
        );
        let screen = Size::new(1440.0, 900.0);
        let (area, offset) = overview_area(Shape::TOP, screen);
        assert_eq!((area.h, offset), (800.0, (0, 32)));
        assert_eq!(overview_area(Shape::default(), screen).1, (0, -844));
    }
}
