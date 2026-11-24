//! The phone-size shell (DESKTOP-SPEC §5.5; boards CompactHome, CompactApps, CompactQuick):
//! a status bar at the top, the home screen behind the apps, a gesture bar at the bottom;
//! all apps, the search and the quick settings open over the home screen. Geometry of the
//! boards: 360 × 800, status bar 32, gesture bar 44, content padding 12 × 16, four columns.

pub mod apps;
pub mod home;
pub mod quick;
pub mod search;

use super::{new_ui, Look, Target, View};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Length, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{size, ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Label, Text};

/// Height of the status bar (board: 32).
pub const STATUS_HEIGHT: f32 = Space::S32.points();
/// Height of the gesture bar (board: 44).
pub const GESTURE_HEIGHT: f32 = size::TARGET_TOUCH_MIN;
/// Length of the gesture bar's handle (board: 112).
pub const HOME_HANDLE: f32 = 28.0 * Space::S4.points();
/// Length of a sheet's handle (board: 48).
pub const SHEET_HANDLE: f32 = Space::S48.points();
/// App cells per row (board: `repeat(4, 1fr)`).
pub const COLUMNS: usize = 4;

/// What the status bar shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusText {
    /// The time (empty: no time, as on the lock screen).
    pub time: String,
    /// Status icons in reading order.
    pub icons: Vec<IconId>,
    /// The battery level in percent, if there is a battery.
    pub battery: Option<u8>,
    /// The quick settings are open.
    pub open: bool,
}

/// The space between the bars: the screen without status and gesture bar.
pub fn content_area(screen: Size) -> Size {
    Size::new(
        screen.w,
        (screen.h - STATUS_HEIGHT - GESTURE_HEIGHT).max(0.0),
    )
}

/// A sizing that takes the row's left-over width at a fixed height.
pub(crate) fn row_of(height: f32) -> Sizing {
    Sizing {
        width: Length::Auto,
        height: Length::Points(height),
        grow: 1,
    }
}

/// Adds the status button (icons and battery level) to `row`; operable only when `mark`.
pub(crate) fn status_button(
    view: &mut View,
    row: NodeId,
    text: &StatusText,
    mark: bool,
) -> Result<NodeId, UiError> {
    let level = text
        .battery
        .map(|p| Label::key_with("desk.percent", &[("value", &p.to_string())]));
    let brick = Brick::new(
        BrickKind::PhoneStatus {
            icons: text.icons.clone(),
            level,
            open: text.open,
        },
        Label::key("desk.bar.quick"),
    );
    let id = view
        .ui
        .add_sized(row, brick, Sizing::AUTO.with_height(STATUS_HEIGHT))?;
    if mark {
        view.mark(id, Target::Status);
    } else {
        view.ui.set_disabled(id, true)?;
    }
    Ok(id)
}

/// The status bar: the time at the start, the status button at the end (CLICKS
/// `mobile.status`; a drag down on it opens the quick settings like a tap).
pub fn status_bar(
    text: &StatusText,
    look: Look,
    width: f32,
    tr: &Translator,
) -> Result<View, UiError> {
    let mut ui = new_ui(look, Size::new(width, STATUS_HEIGHT), tr)?;
    ui.set_background(ColorRole::Ground);
    let flex = Flex::row()
        .main(MainAlign::SpaceBetween)
        .cross(CrossAlign::Center)
        .padding(Padding {
            start: Some(Space::S12),
            end: Some(Space::S12),
            top: None,
            bottom: None,
        });
    ui.set_root_flex(flex);
    let mut view = View::new(ui);
    let root = view.ui.root();
    let time = Text::new(Label::data(&text.time))
        .spec(TextSpec::new(TypeSize::S12).weight(Weight::Semibold));
    view.ui.add(root, time)?;
    let id = status_button(&mut view, root, text, true)?;
    view.ui.set_draggable(id, true)?;
    Ok(view)
}

/// The gesture bar (CLICKS `mobile.home`): tap or swipe up = home screen; held = recent
/// apps.
pub fn gesture_bar(look: Look, width: f32, tr: &Translator) -> Result<View, UiError> {
    let mut ui = new_ui(look, Size::new(width, GESTURE_HEIGHT), tr)?;
    ui.set_background(ColorRole::Ground);
    ui.set_root_flex(Flex::row());
    let mut view = View::new(ui);
    let root = view.ui.root();
    let handle = Brick::new(
        BrickKind::Handle { width: HOME_HANDLE },
        Label::key("desk.mobile.home"),
    );
    let id = view.ui.add_sized(root, handle, row_of(GESTURE_HEIGHT))?;
    view.mark(id, Target::Home);
    view.ui.set_draggable(id, true)?;
    Ok(view)
}

/// A row of app cells with [`COLUMNS`] equal columns (short rows keep their columns).
pub(crate) fn cell_rows(
    view: &mut View,
    parent: NodeId,
    apps: &[crate::model::apps::AppInfo],
    mut each: impl FnMut(&mut View, NodeId, &crate::model::apps::AppInfo) -> Result<(), UiError>,
) -> Result<(), UiError> {
    for chunk in apps.chunks(COLUMNS) {
        let row = view.ui.add(parent, Flex::row().gap(Space::S8))?;
        for app in chunk {
            let cell = Brick::new(
                BrickKind::PhoneCell {
                    icon: app.icon,
                    open: false,
                },
                app.label(),
            );
            let id = view.ui.add_sized(row, cell, Sizing::grow(1))?;
            each(view, id, app)?;
        }
        for _ in chunk.len()..COLUMNS {
            view.ui.add_sized(row, Flex::row(), Sizing::grow(1))?;
        }
    }
    Ok(())
}

/// Marks an app cell: starts the app, with the app's menu on a long press; disabled with
/// the reason while the launcher does not answer and the app has no window.
pub(crate) fn app_cell(
    view: &mut View,
    id: NodeId,
    d: &crate::model::Desktop,
    app: &crate::model::apps::AppInfo,
) -> Result<(), UiError> {
    view.mark(id, Target::AppItem(app.id.clone()));
    view.ui
        .set_context_menu(id, Some(super::apps::menu_for(d, app, true)))?;
    // Dragged onto another app: moved there (home screen) or a folder (all apps).
    view.ui.set_draggable(id, true)?;
    view.ui.set_drop_target(id, true)?;
    if !d.presence.launcher && !d.windows.iter().any(|w| w.app_id == app.id) {
        view.ui.set_disabled(id, true)?;
        view.ui
            .set_tooltip(id, Some(Label::key("desk.reason.no_launcher")))?;
    }
    Ok(())
}
