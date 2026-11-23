//! Snapshot cases of wave 4: states that were missing (loading, error, mixed), the field
//! label, and the primitives of the desktop shell (password field, table, tree, sidebar,
//! header bar, toast, anchored panel).

use super::key;
use super::sheet::{Sheet, State};
use ultar_ui::event::Key;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, Checkbox, Dialog, HeaderBar, IconButton, Label, List, ListItem, Slider, Table,
    TextField, Toast, Tone, Tree, TreeItem,
};
use ultar_ui::Size;

fn k(key: &str) -> Label {
    Label::key(key)
}

fn missing_states(sheet: &mut Sheet) {
    let row = Size::new(300.0, 96.0);
    let states = [State::Loading, State::Error];
    sheet.states("checkbox-on", row, &states, &|ui| {
        let root = ui.root();
        ui.add(root, Checkbox::new(k("demo.check.updates"), true))
    });
    sheet.shoot(
        "checkbox-mixed-normal",
        row,
        &|ui| {
            let root = ui.root();
            ui.add(root, Checkbox::new(k("demo.check.updates"), false).mixed())
        },
        State::Normal,
    );
    sheet.states("slider", Size::new(260.0, 90.0), &states, &|ui| {
        let root = ui.root();
        let slider = Slider::new(k("demo.brightness"), 0.0, 100.0, 5.0).value(30.0);
        ui.add(root, slider)
    });
    sheet.states("listitem-plain", Size::new(360.0, 80.0), &states, &|ui| {
        let root = ui.root();
        let list = ui.add_sized(root, List::new(), Sizing::width(320.0))?;
        let item = ListItem::new(k("demo.list.guest")).icon(IconId::Wifi);
        ui.add(list, item)
    });
    sheet.states(
        "textfield-labelled",
        Size::new(320.0, 110.0),
        &[State::Normal, State::Focused, State::Error],
        &|ui| {
            let root = ui.root();
            ui.add(root, TextField::new(k("demo.device")).labelled())
        },
    );
}

fn password(sheet: &mut Sheet) {
    let states = [State::Normal, State::Focused, State::Disabled, State::Error];
    sheet.states("password", Size::new(320.0, 90.0), &states, &|ui| {
        let root = ui.root();
        ui.add(
            root,
            TextField::password(k("demo.password")).text("sesame 42"),
        )
    });
    sheet.shoot(
        "password-shown-normal",
        Size::new(320.0, 64.0),
        &|ui| {
            let root = ui.root();
            let id = ui.add(
                root,
                TextField::password(k("demo.password")).text("sesame 42"),
            )?;
            // Laid out first: the press must land on the eye button (round 4, w-m4).
            ui.layout();
            let rect = ui.rect(id).unwrap_or_default();
            let pos = ultar_ui::Point::new(rect.right() - 8.0, rect.y + rect.h / 2.0);
            if ui.text(id).is_ok() {
                ui.handle(ultar_ui::event::Event::PointerDown {
                    pos,
                    button: ultar_ui::event::PointerButton::Primary,
                });
            }
            Ok(id)
        },
        State::Normal,
    );
}

fn table(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let columns = vec![
        k("demo.table.name"),
        k("demo.table.size"),
        k("demo.table.kind"),
    ];
    let row =
        |name: &str, size: &str, kind: &str| vec![Label::data(name), Label::data(size), k(kind)];
    let rows = vec![
        row("تقرير.odt", "24 KB", "demo.kind.doc"),
        row("photo.jpg", "3.1 MB", "demo.kind.image"),
        row("خطة 2027.odt", "88 KB", "demo.kind.doc"),
    ];
    let mut table = Table::new(k("demo.table"), columns).rows(rows);
    table.sort = Some((0, true));
    table.selected = Some(2);
    ui.add_sized(root, table, Sizing::width(420.0))
}

fn tree(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let items = vec![
        TreeItem::new(k("demo.tree.home"), 0)
            .icon(IconId::Home)
            .expanded(),
        TreeItem::new(k("demo.tree.docs"), 1)
            .icon(IconId::Folder)
            .expanded(),
        TreeItem::new(k("demo.tree.work"), 2).icon(IconId::Folder),
        TreeItem::new(k("demo.tree.pictures"), 1).icon(IconId::Folder),
        TreeItem::new(Label::data("2026"), 2),
    ];
    let mut tree = Tree::new(k("demo.tree"), items);
    tree.selected = Some(1);
    ui.add_sized(root, tree, Sizing::width(280.0))
}

fn sidebar(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let nav = ui.add_sized(
        root,
        List::navigation().name(k("demo.nav")),
        Sizing::width(220.0),
    )?;
    let item = |key: &str, icon| ListItem::new(k(key)).icon(icon);
    ui.add(nav, item("demo.nav.network", IconId::Network))?;
    ui.add(nav, item("demo.nav.display", IconId::Monitor).selected())?;
    ui.add(nav, item("demo.nav.sound", IconId::Volume))?;
    Ok(nav)
}

fn header(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    let bar = ui.add_sized(root, HeaderBar::new(k("demo.header")), Sizing::width(480.0))?;
    let start = ui.add(bar, Flex::row().gap(Space::S4))?;
    ui.add(start, IconButton::new(IconId::Back, k("demo.back")))?;
    let end = ui.add(bar, Flex::row().gap(Space::S4))?;
    ui.add(end, IconButton::new(IconId::Search, k("demo.search")))?;
    ui.add(end, IconButton::new(IconId::More, k("demo.more")))?;
    Ok(bar)
}

fn data(sheet: &mut Sheet) {
    let states = [
        State::Normal,
        State::Hovered,
        State::Focused,
        State::Disabled,
    ];
    sheet.states("table", Size::new(460.0, 190.0), &states, &table);
    sheet.states("tree", Size::new(320.0, 220.0), &states, &tree);
    sheet.states(
        "sidebar",
        Size::new(260.0, 170.0),
        &[State::Normal, State::Hovered],
        &sidebar,
    );
    sheet.shoot(
        "headerbar-normal",
        Size::new(512.0, 72.0),
        &header,
        State::Normal,
    );
}

fn overlays(sheet: &mut Sheet) {
    sheet.shoot(
        "toast-normal",
        Size::new(520.0, 200.0),
        &|ui| {
            let root = ui.root();
            let id = ui.add(root, Button::new(k("demo.save")))?;
            ui.show_toast(Toast::new(1, Tone::Success, k("demo.toast.saved")))?;
            let undo =
                Toast::new(2, Tone::Info, k("demo.toast.deleted")).action(k("demo.toast.undo"));
            ui.show_toast(undo)?;
            Ok(id)
        },
        State::Normal,
    );
    sheet.shoot(
        "panel-open-normal",
        Size::new(560.0, 320.0),
        &|ui| {
            let root = ui.root();
            ui.set_root_flex(
                Flex::column()
                    .main(ultar_ui::layout::MainAlign::End)
                    .cross(ultar_ui::layout::CrossAlign::End),
            );
            let anchor = ui.add(root, IconButton::new(IconId::Wifi, k("demo.panel")))?;
            let panel = ui.add(root, Dialog::panel(k("demo.panel")))?;
            ui.add(panel, ultar_ui::widgets::Switch::new(k("demo.night"), true))?;
            key(ui, Key::Tab);
            ui.open_panel(panel, anchor)?;
            Ok(panel)
        },
        State::Normal,
    );
}

/// Runs all wave-4 cases.
pub fn all(sheet: &mut Sheet) {
    missing_states(sheet);
    password(sheet);
    data(sheet);
    overlays(sheet);
}
