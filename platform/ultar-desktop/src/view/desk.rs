//! The icons on the desktop background and the rename dialog of a shortcut (DESKTOP-SPEC
//! §5.1 "Arbeitsfläche", CLICKS `desk.icon`): the home folder, the drives, the app shortcuts
//! and the desktop folder's items stand in columns from the corner of
//! `desktop.icons.start_corner`, 24 from the edges and 8 apart, in the size of
//! `desktop.icons.size`; a full column continues in the next one.

use super::geometry::{self, Placement};
use super::{card_sheet, new_ui, Look, Target, View};
use crate::model::desk_files;
use crate::model::places::{self, DeskItem, MAX_NAME};
use crate::model::Desktop;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
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

/// The icons that are shown, in their order: the home folder, the mounted drives, the app
/// shortcuts (only those whose app is installed), then the desktop folder's items; none
/// while `desktop.icons.enabled` is off.
pub fn shown(d: &Desktop) -> Vec<DeskItem> {
    if !d.prefs.desk.shown {
        return Vec::new();
    }
    let mut items = vec![DeskItem::Home];
    let drives = d.svc.drives.as_ref().map_or(0, Vec::len);
    items.extend((0..drives).map(DeskItem::Drive));
    items.extend(
        (0..d.places.shortcuts.len())
            .filter(|i| {
                let app = &d.places.shortcuts[*i].app;
                d.catalog.iter().any(|a| &a.id == app)
            })
            .map(DeskItem::Shortcut),
    );
    items.extend((0..d.places.files.len()).map(DeskItem::File));
    items
}

/// Icon and name of an icon on the desktop.
pub fn look_of(d: &Desktop, item: DeskItem, tr: &Translator) -> Option<(IconId, Label)> {
    Some(match item {
        DeskItem::Home => (IconId::Home, Label::key("desk.desk.home")),
        DeskItem::Drive(i) => {
            let drive = d.svc.drives.as_ref()?.get(i)?;
            (IconId::Usb, Label::data(&drive.name))
        }
        DeskItem::Shortcut(i) => {
            let shortcut = d.places.shortcuts.get(i)?;
            let app = d.catalog.iter().find(|a| a.id == shortcut.app)?;
            let name = places::shown_name(shortcut, &d.catalog, tr);
            (app.icon, Label::data(&name))
        }
        DeskItem::File(i) => {
            let file = d.places.files.get(i)?;
            (desk_files::icon(file), Label::data(&file.name))
        }
    })
}

/// The size of one icon's cell in this look.
fn cell(look: Look, side: u16, tr: &Translator) -> Result<Size, UiError> {
    let mut probe = new_ui(look, Size::new(1000.0, 1000.0), tr)?;
    probe.set_root_flex(Flex::column().cross(CrossAlign::Start));
    let root = probe.root();
    let brick = BrickKind::DeskIcon {
        icon: IconId::Folder,
        side,
        selected: false,
    };
    probe.add(root, Brick::new(brick, Label::data("")))?;
    Ok(probe.content_size())
}

/// One cell of the layout: an icon, or the place an app dragged out of the app grid would
/// take.
#[derive(Clone, Copy)]
enum Slot<'a> {
    Item(DeskItem),
    Ghost(&'a str),
}

/// Puts the icons into `area` (a growing child of the background's overlay) of `height`
/// points.
pub fn shortcuts(
    view: &mut View,
    area: NodeId,
    d: &Desktop,
    (look, height): (Look, f32),
    tr: &Translator,
) -> Result<(), UiError> {
    let mut slots: Vec<Slot> = shown(d).into_iter().map(Slot::Item).collect();
    if let Some(app) = d.places.dragging.as_deref().filter(|_| d.prefs.desk.shown) {
        slots.push(Slot::Ghost(app));
    }
    if slots.is_empty() {
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
    let mut columns: Vec<&[Slot]> = slots.chunks(per_column).collect();
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
        let mut cells = column.to_vec();
        if desk.corner.bottom() {
            // The first one stands in the bottom corner.
            cells.reverse();
        }
        for slot in cells {
            match slot {
                Slot::Item(item) => icon(view, node, d, item, (side, tr))?,
                Slot::Ghost(app) => ghost(view, node, d, app, side)?,
            }
        }
    }
    Ok(())
}

fn icon(
    view: &mut View,
    column: NodeId,
    d: &Desktop,
    item: DeskItem,
    (side, tr): (u16, &Translator),
) -> Result<(), UiError> {
    let Some((icon, label)) = look_of(d, item, tr) else {
        return Ok(());
    };
    let brick = BrickKind::DeskIcon {
        icon,
        side,
        selected: d.places.selected == Some(item),
    };
    let id = view.ui.add(column, Brick::new(brick, label.clone()))?;
    view.ui.set_tooltip(id, Some(label))?;
    view.mark(id, Target::Desk(item));
    Ok(())
}

/// Where the dragged app's shortcut would go: its icon, faint (disabled), not operable.
fn ghost(
    view: &mut View,
    column: NodeId,
    d: &Desktop,
    app: &str,
    side: u16,
) -> Result<(), UiError> {
    let Some(info) = d.catalog.iter().find(|a| a.id == app) else {
        return Ok(());
    };
    let brick = BrickKind::DeskIcon {
        icon: info.icon,
        side,
        selected: true,
    };
    let id = view.ui.add(column, Brick::new(brick, info.label()))?;
    view.ui.set_disabled(id, true)
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
