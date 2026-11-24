//! The one bar at the bottom (DESKTOP-SPEC §11, board `bottombar`): start – trash; centre –
//! overview, all apps, search, pinned and running apps; end – input language, status, clock.
//! Height `size.bar_height`, background `bar`, a hairline on top, inner padding 12; the
//! two side groups are equally wide so that the centre group is centred (CSS `flex: 1`).

use super::{new_ui, Look, Target, View};
use crate::model::apps::BarApp;
use crate::model::Desktop;
use crate::model::Open;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{size, ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Label, Picture};

/// What the bar shows besides the desktop state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarText {
    /// The time line.
    pub time: String,
    /// The date line.
    pub date: String,
    /// Status icons (network, sound, battery – only what says something).
    pub status: Vec<IconId>,
}

const GAP: Space = Space::S8;
const END_GAP: Space = Space::S4;

/// Width of the centre group (overview, all apps, search, tiles).
pub fn centre_width(tiles: usize) -> f32 {
    let items = 3 + tiles;
    2.0 * size::TARGET_TOUCH_MIN
        + super::geometry::SEARCH_WIDTH
        + tiles as f32 * size::TARGET_TOUCH_MIN
        + (items.saturating_sub(1)) as f32 * GAP.points()
}

fn app_name(tile: &BarApp, tr: &Translator) -> Label {
    let name = tile.app.display_name(tr);
    if tile.windows.is_empty() {
        Label::data(&name)
    } else {
        Label::key_with("desk.bar.running", &[("app", &name)])
    }
}

/// Builds the bar for a screen `width`.
pub fn build(
    d: &Desktop,
    text: &BarText,
    look: Look,
    width: f32,
    tr: &Translator,
) -> Result<View, UiError> {
    let mut ui = new_ui(look, Size::new(width, size::BAR_HEIGHT), tr)?;
    ui.set_background(ColorRole::Bar);
    let root = ui.root();
    ui.add_sized(
        root,
        Brick::new(BrickKind::Rule, Label::data("")),
        Sizing::AUTO,
    )?;
    let row_flex = Flex::row()
        .padding(Padding {
            start: Some(Space::S12),
            end: Some(Space::S12),
            top: None,
            bottom: None,
        })
        .cross(CrossAlign::Center);
    let row = ui.add_sized(root, row_flex, Sizing::grow(1))?;
    let mut view = View::new(ui);
    let tiles = d.bar_apps();
    let side = ((width - 2.0 * Space::S12.points() - centre_width(tiles.len())) / 2.0).max(0.0);
    start_group(&mut view, row, side)?;
    centre_group(&mut view, row, d, &tiles, tr)?;
    end_group(&mut view, row, d, text, side)?;
    Ok(view)
}

fn start_group(view: &mut View, row: NodeId, side: f32) -> Result<(), UiError> {
    let group = view
        .ui
        .add_sized(row, Flex::row().gap(GAP), Sizing::width(side))?;
    let trash = Brick::new(
        BrickKind::Tile {
            icon: IconId::Bin,
            running: 0,
            active: false,
        },
        Label::key("desk.bar.trash"),
    );
    let id = view.ui.add(group, trash)?;
    view.mark(id, Target::Trash);
    Ok(())
}

fn centre_group(
    view: &mut View,
    row: NodeId,
    d: &Desktop,
    tiles: &[BarApp],
    tr: &Translator,
) -> Result<(), UiError> {
    let group = view.ui.add(row, Flex::row().gap(GAP))?;
    let overview = Brick::new(
        BrickKind::Square {
            picture: Picture::Mark,
            open: d.open == Open::Overview,
        },
        Label::key("desk.bar.overview"),
    );
    let id = view.ui.add(group, overview)?;
    view.mark(id, Target::Overview);
    let apps = Brick::new(
        BrickKind::Square {
            picture: Picture::Icon(IconId::Grid),
            open: matches!(d.open, Open::Apps(_)),
        },
        Label::key("desk.bar.apps"),
    );
    let id = view.ui.add(group, apps)?;
    view.mark(id, Target::AllApps);
    let search = Brick::new(
        BrickKind::SearchPill {
            width: super::geometry::SEARCH_WIDTH,
        },
        Label::key("desk.bar.search"),
    );
    let id = view.ui.add(group, search)?;
    view.mark(id, Target::Search);
    for (i, tile) in tiles.iter().enumerate() {
        let brick = Brick::new(
            BrickKind::Tile {
                icon: tile.app.icon,
                running: tile.windows.len().min(usize::from(u8::MAX)) as u8,
                active: tile.active(),
            },
            app_name(tile, tr),
        );
        let id = view.ui.add(group, brick)?;
        view.mark(id, Target::App(i));
        if !d.presence.launcher && tile.windows.is_empty() {
            view.ui.set_disabled(id, true)?;
            view.ui
                .set_tooltip(id, Some(Label::key("desk.reason.no_launcher")))?;
        }
    }
    Ok(())
}

fn end_group(
    view: &mut View,
    row: NodeId,
    d: &Desktop,
    text: &BarText,
    side: f32,
) -> Result<(), UiError> {
    let flex = Flex::row().gap(END_GAP).main(MainAlign::End);
    let group = view.ui.add_sized(row, flex, Sizing::width(side))?;
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
        let id = view.ui.add(group, chip)?;
        view.mark(id, Target::Keyboard);
    }
    let status = Brick::new(
        BrickKind::Status {
            icons: text.status.clone(),
            open: matches!(d.open, Open::Quick(_)),
        },
        Label::key("desk.bar.quick"),
    );
    let id = view.ui.add(group, status)?;
    view.mark(id, Target::Status);
    let clock = Brick::new(
        BrickKind::Clock {
            date: Label::data(&text.date),
            open: d.open == Open::Calendar,
        },
        Label::data(&text.time),
    )
    .named(Label::key_with(
        "desk.bar.clock",
        &[("date", &text.date), ("time", &text.time)],
    ));
    let id = view.ui.add(group, clock)?;
    view.mark(id, Target::Clock);
    Ok(())
}
