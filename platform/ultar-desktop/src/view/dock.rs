//! The variant "bar at the top" (board DesktopTop; DESKTOP-SPEC §11): a top bar of 32 with
//! the overview button and the active app's menu at the start, the date and time in the
//! middle, input language and status at the end – and a floating dock at the bottom with
//! "all apps", the pinned and running apps and the trash.

use super::bar::BarText;
use super::{new_ui, Look, Target, View};
use crate::model::apps::BarApp;
use crate::model::{Desktop, Open};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::text::Weight;
use ultar_ui::tokens::{size, ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Card, Label, Picture};

/// Height of the top bar (board: 32).
pub const TOP_HEIGHT: f32 = size::TARGET_POINTER_MIN;
/// Height of the dock (board: 56).
pub const DOCK_HEIGHT: f32 = size::BAR_HEIGHT;
/// Distance of the dock from the bottom edge (board: 12).
pub const DOCK_BOTTOM: Space = Space::S12;
/// What the dock's panel reserves at the bottom edge.
pub const DOCK_PANEL: f32 = DOCK_HEIGHT + DOCK_BOTTOM.points();
/// Height of the separators in the dock (board: 24).
const SEPARATOR: f32 = Space::S24.points();

/// The app that has the focused window, if any (the top bar shows its menu).
pub fn active_app(d: &Desktop) -> Option<BarApp> {
    d.bar_apps().into_iter().find(BarApp::active)
}

fn side(view: &mut View, row: NodeId, main: MainAlign, gap: Space) -> Result<NodeId, UiError> {
    let flex = Flex::row().gap(gap).main(main).cross(CrossAlign::Center);
    view.ui.add_sized(row, flex, Sizing::grow(1))
}

/// Builds the top bar for a screen `width`.
pub fn top_bar(
    d: &Desktop,
    text: &BarText,
    look: Look,
    width: f32,
    tr: &Translator,
) -> Result<View, UiError> {
    let mut ui = new_ui(look, Size::new(width, TOP_HEIGHT), tr)?;
    ui.set_background(ColorRole::Bar);
    ui.set_root_flex(Flex::column());
    let root = ui.root();
    let flex = Flex::row().cross(CrossAlign::Center).padding(Padding {
        start: Some(Space::S8),
        end: Some(Space::S8),
        top: None,
        bottom: None,
    });
    let row = ui.add_sized(root, flex, Sizing::grow(1))?;
    ui.add(root, Brick::new(BrickKind::Rule, Label::data("")))?;
    let mut view = View::new(ui);
    start_group(&mut view, row, d, tr)?;
    let clock = Brick::new(
        BrickKind::BarButton {
            pad: Space::S12,
            weight: Weight::Medium,
            open: d.open == Open::Calendar,
        },
        Label::key_with(
            "desk.top.clock",
            &[("date", &text.date), ("time", &text.time)],
        ),
    )
    .named(Label::key_with(
        "desk.bar.clock",
        &[("date", &text.date), ("time", &text.time)],
    ));
    let id = view.ui.add(row, clock)?;
    view.mark(id, Target::Clock);
    let end = side(&mut view, row, MainAlign::End, Space::S4)?;
    end_group(&mut view, end, d, text)?;
    Ok(view)
}

/// The start of the top bar: the overview button and the active app's menu.
fn start_group(view: &mut View, row: NodeId, d: &Desktop, tr: &Translator) -> Result<(), UiError> {
    let start = side(view, row, MainAlign::Start, Space::S8)?;
    let overview = Brick::new(
        BrickKind::Square {
            picture: Picture::Mark,
            open: d.open == Open::Overview,
        },
        Label::key("desk.bar.overview"),
    );
    let square = Sizing::width(TOP_HEIGHT).with_height(TOP_HEIGHT);
    let id = view.ui.add_sized(start, overview, square)?;
    view.mark(id, Target::Overview);
    if let Some(app) = active_app(d) {
        let menu = Brick::new(
            BrickKind::BarButton {
                pad: Space::S8,
                weight: Weight::Semibold,
                open: d.open == Open::AppMenu,
            },
            app.app.label(),
        )
        .named(Label::key_with(
            "desk.top.appmenu",
            &[("app", &app.app.display_name(tr))],
        ));
        let id = view.ui.add(start, menu)?;
        view.mark(id, Target::AppMenu);
    }
    Ok(())
}

fn end_group(view: &mut View, end: NodeId, d: &Desktop, text: &BarText) -> Result<(), UiError> {
    if d.shows_keyboard() {
        let active = d
            .layouts
            .names
            .get(usize::from(d.layouts.active))
            .cloned()
            .unwrap_or_default();
        let chip = Brick::new(
            BrickKind::Chip,
            Label::key(format!("desk.kbd.short.{active}")),
        )
        .named(Label::key(format!("desk.kbd.name.{active}")));
        let id = view.ui.add(end, chip)?;
        view.mark(id, Target::Keyboard);
    }
    let status = Brick::new(
        BrickKind::Status {
            icons: text.status.clone(),
            open: matches!(d.open, Open::Quick(_)),
        },
        Label::key("desk.bar.quick"),
    );
    let id = view.ui.add(end, status)?;
    view.mark(id, Target::Status);
    Ok(())
}

fn separator(view: &mut View, card: NodeId) -> Result<(), UiError> {
    let rule = Brick::new(BrickKind::Rule, Label::data(""));
    view.ui.add_sized(
        card,
        rule,
        Sizing::width(size::BORDER).with_height(SEPARATOR),
    )?;
    Ok(())
}

/// Builds the dock's panel for a screen `width`: the floating dock centred in it.
pub fn dock(d: &Desktop, look: Look, width: f32, tr: &Translator) -> Result<View, UiError> {
    let mut ui = new_ui(look, Size::new(width, DOCK_PANEL), tr)?;
    ui.set_transparent_background();
    ui.set_root_flex(Flex::row().main(MainAlign::Center).cross(CrossAlign::Start));
    let flex = Flex::row()
        .gap(Space::S8)
        .cross(CrossAlign::Center)
        .padding(Padding {
            start: Some(Space::S8),
            end: Some(Space::S8),
            top: None,
            bottom: None,
        });
    let root = ui.root();
    let card = ui.add_sized(
        root,
        Card::new().flex(flex).dock(),
        Sizing::AUTO.with_height(DOCK_HEIGHT),
    )?;
    let mut view = View::new(ui);
    let apps = Brick::new(
        BrickKind::Square {
            picture: Picture::Icon(IconId::Grid),
            open: matches!(d.open, Open::Apps(_)),
        },
        Label::key("desk.bar.apps"),
    );
    let id = view.ui.add(card, apps)?;
    view.mark(id, Target::AllApps);
    separator(&mut view, card)?;
    for (i, tile) in d.bar_apps().iter().enumerate() {
        let id = super::bar::app_tile(&mut view, card, d, tile, tr)?;
        view.mark(id, Target::App(i));
    }
    separator(&mut view, card)?;
    let trash = Brick::new(
        BrickKind::Tile {
            icon: IconId::Bin,
            running: 0,
            active: false,
        },
        Label::key("desk.bar.trash"),
    );
    let id = view.ui.add(card, trash)?;
    view.mark(id, Target::Trash);
    super::bar::drives(&mut view, card, d)?;
    Ok(view)
}
