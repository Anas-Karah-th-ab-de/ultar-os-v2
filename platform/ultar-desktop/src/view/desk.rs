//! The desktop's shortcuts on the background and the rename dialog of one (DESKTOP-SPEC §5.1
//! "Arbeitsfläche"): the shortcuts stand in columns from the corner of
//! `desktop.icons.start_corner`, 24 from the edges and 8 apart, in the size of
//! `desktop.icons.size`; a full column continues in the next one.

use super::geometry::{self, Placement};
use super::{card_sheet, new_ui, Look, Target, View};
use crate::model::places::{self, MAX_NAME};
use crate::model::Desktop;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Button, ButtonVariant, Label, Text, TextField};

/// Distance of the shortcuts from the edges of the work area.
const EDGE: Space = Space::S24;
/// Distance between shortcuts.
const BETWEEN: Space = Space::S8;
/// Width of the rename dialog.
const RENAME_WIDTH: f32 = 100.0 * Space::S4.points();

/// The shortcuts that are shown, by index: none while `desktop.icons.enabled` is off, and
/// only those whose app is installed.
pub fn shown(d: &Desktop) -> Vec<usize> {
    if !d.prefs.desk.shown {
        return Vec::new();
    }
    (0..d.places.shortcuts.len())
        .filter(|i| {
            let app = &d.places.shortcuts[*i].app;
            d.catalog.iter().any(|a| &a.id == app)
        })
        .collect()
}

/// The size of one shortcut's cell in this look.
fn cell(look: Look, side: u16, tr: &Translator) -> Result<Size, UiError> {
    let mut probe = new_ui(look, Size::new(1000.0, 1000.0), tr)?;
    probe.set_root_flex(Flex::column().cross(CrossAlign::Start));
    let root = probe.root();
    let brick = BrickKind::DeskIcon {
        icon: ultar_ui::icon::IconId::Folder,
        side,
        selected: false,
    };
    probe.add(root, Brick::new(brick, Label::data("")))?;
    Ok(probe.content_size())
}

/// Puts the shortcuts into `area` (a growing child of the background's overlay) of
/// `height` points.
pub fn shortcuts(
    view: &mut View,
    area: NodeId,
    d: &Desktop,
    (look, height): (Look, f32),
    tr: &Translator,
) -> Result<(), UiError> {
    let list = shown(d);
    if list.is_empty() {
        return Ok(());
    }
    let desk = d.prefs.desk;
    let side = desk.size.side();
    let cell = cell(look, side, tr)?;
    let room = height - 2.0 * EDGE.points() + BETWEEN.points();
    let per_column = ((room / (cell.h + BETWEEN.points())).floor() as usize).max(1);
    let main = if desk.corner.end() {
        MainAlign::End
    } else {
        MainAlign::Start
    };
    let layer = Flex::row()
        .gap(BETWEEN)
        .padding(Padding::all(EDGE))
        .main(main)
        .cross(CrossAlign::Stretch);
    let layer = view.ui.add_sized(area, layer, Sizing::grow(1))?;
    let mut columns: Vec<&[usize]> = list.chunks(per_column).collect();
    if desk.corner.end() {
        // The first column stands at the end edge.
        columns.reverse();
    }
    for column in columns {
        let flex =
            Flex::column()
                .gap(BETWEEN)
                .cross(CrossAlign::Center)
                .main(if desk.corner.bottom() {
                    MainAlign::End
                } else {
                    MainAlign::Start
                });
        let node = view.ui.add_sized(layer, flex, Sizing::AUTO)?;
        let mut items = column.to_vec();
        if desk.corner.bottom() {
            // The first one stands in the bottom corner.
            items.reverse();
        }
        for i in items {
            icon(view, node, d, i, (side, tr))?;
        }
    }
    Ok(())
}

fn icon(
    view: &mut View,
    column: NodeId,
    d: &Desktop,
    index: usize,
    (side, tr): (u16, &Translator),
) -> Result<(), UiError> {
    let shortcut = &d.places.shortcuts[index];
    let Some(app) = d.catalog.iter().find(|a| a.id == shortcut.app) else {
        return Ok(());
    };
    let name = places::shown_name(shortcut, &d.catalog, tr);
    let brick = BrickKind::DeskIcon {
        icon: app.icon,
        side,
        selected: d.places.selected == Some(index),
    };
    let id = view.ui.add(column, Brick::new(brick, Label::data(&name)))?;
    view.ui.set_tooltip(id, Some(Label::data(&name)))?;
    view.mark(id, Target::Shortcut(index));
    Ok(())
}

/// The rename dialog of shortcut `index`: its name in a field (Enter renames), Cancel and
/// Rename. An empty name gives it back the app's name.
pub fn rename(
    d: &Desktop,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    card_sheet(
        look,
        screen,
        RENAME_WIDTH,
        tr,
        geometry::centred,
        |view, card| {
            let title = TextSpec::new(TypeSize::S16).weight(Weight::Semibold);
            view.ui.add(
                card,
                Text::new(Label::key("desk.desk.rename.title")).spec(title),
            )?;
            let field = TextField::new(Label::key("desk.desk.rename.name"))
                .labelled()
                .max_len(MAX_NAME)
                .text(&d.places.draft);
            let field = view.ui.add(card, field)?;
            view.mark(field, Target::RenameField);
            let row = view
                .ui
                .add(card, Flex::row().gap(Space::S8).main(MainAlign::End))?;
            let cancel = view
                .ui
                .add(row, Button::new(Label::key("desk.desk.rename.cancel")))?;
            view.mark(cancel, Target::RenameCancel);
            let save =
                Button::new(Label::key("desk.desk.rename.save")).variant(ButtonVariant::Primary);
            let save = view.ui.add(row, save)?;
            view.mark(save, Target::RenameSave);
            Ok(())
        },
    )
}
