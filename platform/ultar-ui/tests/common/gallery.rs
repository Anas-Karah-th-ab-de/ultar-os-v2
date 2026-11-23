//! A screen holding every wave-3 widget, for behaviour tests and the fuzzer.

use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Badge, Button, ButtonVariant, Checkbox, Dialog, EmptyState, IconButton, InfoBar, Label, List,
    ListItem, Menu, MenuItem, Progress, RadioGroup, Scroll, SearchField, Switch, Tabs, Text, Tone,
};

fn k(key: &str) -> Label {
    Label::key(key)
}

/// Handles of the parts of the gallery.
pub struct Gallery {
    pub search: NodeId,
    pub settings: NodeId,
    pub tabs: NodeId,
    pub scroll: NodeId,
    pub checkbox: NodeId,
    pub radio: NodeId,
    pub list: NodeId,
    pub items: [NodeId; 3],
    pub item_switch: NodeId,
    pub progress: NodeId,
    pub menu_button: NodeId,
    pub status: Status,
    pub modal: Modals,
}

/// The info bar and the empty state of the gallery with their actions.
pub struct Status {
    pub bar: NodeId,
    pub bar_action: NodeId,
    pub empty: NodeId,
    pub empty_action: NodeId,
}

/// The dialog and the sheet of the gallery with their buttons.
pub struct Modals {
    pub dialog: NodeId,
    pub cancel: NodeId,
    pub delete: NodeId,
    pub sheet: NodeId,
    pub close: NodeId,
}

/// The file menu used by the menu tests: icons, a shortcut, a submenu, an on/off command,
/// a disabled and a destructive command.
pub fn file_menu() -> Menu {
    let share = Menu::new(vec![
        MenuItem::new(31, k("demo.menu.share.link")),
        MenuItem::new(32, k("demo.menu.share.device")).icon(IconId::Mesh),
    ]);
    Menu::new(vec![
        MenuItem::new(1, k("demo.menu.open"))
            .icon(IconId::Folder)
            .shortcut(Label::data("Ctrl+O")),
        MenuItem::new(2, k("demo.menu.rename")).shortcut(Label::data("F2")),
        MenuItem::new(3, k("demo.menu.share")).submenu(share),
        MenuItem::new(4, k("demo.menu.pin"))
            .checked(true)
            .separated(),
        MenuItem::new(5, k("demo.menu.properties")).disabled(),
        MenuItem::new(6, k("demo.delete"))
            .icon(IconId::Trash)
            .danger()
            .separated(),
    ])
}

/// A list of three items: the first selected with an icon, the second with a sublabel and
/// a trailing switch, the third plain.
pub fn networks(ui: &mut Ui, parent: NodeId) -> Result<(NodeId, [NodeId; 3], NodeId), UiError> {
    let list = ui.add(parent, List::new().selectable().name(k("demo.list")))?;
    let home = ListItem::new(k("demo.list.home"))
        .sublabel(k("demo.list.home.sub"))
        .icon(IconId::Wifi)
        .selected()
        .separator();
    let home = ui.add(list, home)?;
    let office = ListItem::new(k("demo.list.office"))
        .sublabel(k("demo.list.office.sub"))
        .icon(IconId::Wifi)
        .separator();
    let office = ui.add(list, office)?;
    let toggle = ui.add(office, Switch::new(k("demo.list.auto"), true))?;
    let guest = ui.add(list, ListItem::new(k("demo.list.guest")).icon(IconId::Wifi))?;
    Ok((list, [home, office, guest], toggle))
}

/// The dialog of the gallery: a text and the two actions, cancel and delete.
pub fn delete_dialog(ui: &mut Ui) -> Result<(NodeId, NodeId, NodeId), UiError> {
    let root = ui.root();
    let dialog = ui.add(root, Dialog::new(k("demo.dialog.title")))?;
    ui.add(dialog, Text::new(k("demo.dialog.text")))?;
    let actions = ui.add(
        dialog,
        Flex::row()
            .gap(Space::S8)
            .main(ultar_ui::layout::MainAlign::End),
    )?;
    let cancel = ui.add(actions, Button::new(k("demo.cancel")))?;
    let danger = Button::new(k("demo.delete")).variant(ButtonVariant::Danger);
    let delete = ui.add(actions, danger)?;
    ui.set_cancel_action(dialog, Some(cancel))?;
    ui.set_default_action(dialog, Some(delete))?;
    Ok((dialog, cancel, delete))
}

/// The sheet of the gallery: a text and a close button.
pub fn share_sheet(ui: &mut Ui) -> Result<(NodeId, NodeId), UiError> {
    let root = ui.root();
    let sheet = ui.add(root, Dialog::sheet(k("demo.sheet.title")))?;
    ui.add(sheet, Text::new(k("demo.sheet.text")))?;
    let close = ui.add(sheet, Button::new(k("demo.close")))?;
    ui.set_cancel_action(sheet, Some(close))?;
    Ok((sheet, close))
}

fn top_bar(ui: &mut Ui) -> Result<(NodeId, NodeId, NodeId), UiError> {
    let root = ui.root();
    let bar = ui.add(root, Flex::row().gap(Space::S8))?;
    let field = SearchField::new(k("demo.search")).placeholder(k("demo.search.hint"));
    let search = ui.add_sized(bar, field, Sizing::grow(1))?;
    let menu_button = ui.add(bar, Button::new(k("demo.menu.button")))?;
    let settings = ui.add(bar, IconButton::new(IconId::Gear, k("demo.settings")))?;
    ui.set_tooltip(settings, Some(k("demo.tip.settings")))?;
    Ok((search, menu_button, settings))
}

fn status_widgets(ui: &mut Ui, scroll: NodeId) -> Result<Status, UiError> {
    let bar = InfoBar::new(Tone::Warning, k("demo.bar.warning")).message(k("demo.bar.warning.msg"));
    let bar = ui.add(scroll, bar)?;
    let bar_action = ui.add(bar, Button::new(k("demo.bar.action")))?;
    let badges = ui.add(scroll, Flex::row().gap(Space::S8))?;
    ui.add(badges, Badge::new(k("demo.badge.new")).tone(Tone::Accent))?;
    let failed = Badge::new(k("demo.badge.failed")).tone(Tone::Danger);
    ui.add(badges, failed.with_icon())?;
    let empty = EmptyState::new(IconId::Folder, k("demo.empty.title")).message(k("demo.empty.msg"));
    let empty = ui.add(scroll, empty)?;
    let empty_action = ui.add(empty, Button::new(k("demo.empty.action")))?;
    Ok(Status {
        bar,
        bar_action,
        empty,
        empty_action,
    })
}

fn choice_widgets(ui: &mut Ui, scroll: NodeId) -> Result<(NodeId, NodeId), UiError> {
    let checkbox = ui.add(scroll, Checkbox::new(k("demo.check.updates"), false))?;
    let options = vec![
        k("demo.radio.dark"),
        k("demo.radio.light"),
        k("demo.radio.auto"),
    ];
    let group = RadioGroup::new(k("demo.radio"), options).selected(0);
    let radio = ui.add(scroll, group)?;
    Ok((checkbox, radio))
}

/// Builds the gallery: a top bar (search, menu button, icon button with tooltip), tabs, and
/// a scroll container with the other widgets (taller than a 720 × 560 surface shows).
pub fn gallery(ui: &mut Ui) -> Result<Gallery, UiError> {
    let pad = Padding::symmetric(Space::S24, Space::S16);
    ui.set_root_flex(Flex::column().padding(pad).gap(Space::S12));
    let root = ui.root();
    let (search, menu_button, settings) = top_bar(ui)?;
    let labels = vec![
        k("demo.tabs.general"),
        k("demo.tabs.network"),
        k("demo.tabs.about"),
    ];
    let tabs = ui.add(root, Tabs::new(k("demo.tabs"), labels))?;
    let content = Scroll::new().flex(Flex::column().gap(Space::S12));
    let scroll = ui.add_sized(root, content.name(k("demo.scroll")), Sizing::grow(1))?;
    let (checkbox, radio) = choice_widgets(ui, scroll)?;
    let (list, items, item_switch) = networks(ui, scroll)?;
    let progress = ui.add(scroll, Progress::new(k("demo.progress")).value(0.4))?;
    let status = status_widgets(ui, scroll)?;
    let (dialog, cancel, delete) = delete_dialog(ui)?;
    let (sheet, close) = share_sheet(ui)?;
    ui.set_context_menu(list, Some(file_menu()))?;
    Ok(Gallery {
        search,
        settings,
        tabs,
        scroll,
        checkbox,
        radio,
        list,
        items,
        item_switch,
        progress,
        menu_button,
        status,
        modal: Modals {
            dialog,
            cancel,
            delete,
            sheet,
            close,
        },
    })
}
