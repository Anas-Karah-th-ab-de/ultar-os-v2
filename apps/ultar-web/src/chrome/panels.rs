//! Everything that opens above the window: the page panel of the tracking-protection
//! button (`browser.shield`), the download list (`browser.downloads`), the browser menu,
//! the settings and the permission prompt of a site. Each is built when it opens and
//! removed when it closes.

use crate::engine::{DownloadId, PermissionKind};
use crate::model::downloads::{Download, State};
use crate::model::permissions::Decision;
use crate::model::settings::Settings;
use crate::url::SearchEngine;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Checkbox, Dialog, EmptyState, Label, Menu, MenuItem, Row, Select,
    Switch, Text,
};

/// Menu item: new tab.
pub const MENU_NEW_TAB: u32 = 1;
/// Menu item: new private tab.
pub const MENU_PRIVATE: u32 = 2;
/// Menu item: downloads.
pub const MENU_DOWNLOADS: u32 = 3;
/// Menu item: the page panel.
pub const MENU_SITE: u32 = 4;
/// Menu item: settings.
pub const MENU_SETTINGS: u32 = 5;

/// The browser menu (keyboard: Menu key, Alt+F or F10; pointer: secondary click on the
/// tab strip or the toolbar).
pub fn menu(on_page: bool) -> Menu {
    let mut site = MenuItem::new(MENU_SITE, Label::key("web.menu.site")).icon(IconId::Shield);
    if !on_page {
        site = site.disabled();
    }
    Menu::new(vec![
        MenuItem::new(MENU_NEW_TAB, Label::key("web.menu.newtab")).icon(IconId::Plus),
        MenuItem::new(MENU_PRIVATE, Label::key("web.menu.private")).icon(IconId::Eye),
        MenuItem::new(MENU_DOWNLOADS, Label::key("web.menu.downloads")).icon(IconId::Download),
        site.separated(),
        MenuItem::new(MENU_SETTINGS, Label::key("web.menu.settings")).icon(IconId::Gear),
    ])
}

/// What the page panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShieldView {
    /// Host of the page.
    pub host: String,
    /// Blocked on this page (`None`: the engine does not count).
    pub blocked: Option<u32>,
    /// Protection for this site.
    pub site_on: bool,
    /// Encrypted connection.
    pub secure: bool,
    /// The site's rights.
    pub rights: Vec<(PermissionKind, Decision)>,
}

/// The nodes of the page panel.
#[derive(Debug, Clone)]
pub struct ShieldPanel {
    /// The panel.
    pub dialog: NodeId,
    /// "Protection for this site".
    pub site: NodeId,
    /// One choice per right.
    pub rights: Vec<(NodeId, PermissionKind)>,
    /// Close.
    pub close: NodeId,
}

fn small(label: Label) -> Text {
    Text::new(label)
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted)
}

fn panel(ui: &mut Ui, title: Label) -> Result<NodeId, UiError> {
    let root = ui.root();
    ui.add_sized(
        root,
        Dialog::panel(title),
        Sizing::width(10.0 * Space::S32.points()),
    )
}

/// Opens the page panel at the tracking-protection button.
pub fn open_shield(ui: &mut Ui, anchor: NodeId, view: &ShieldView) -> Result<ShieldPanel, UiError> {
    let dialog = panel(ui, Label::key("web.panel.title"))?;
    ui.add(dialog, Text::new(Label::data(&view.host)))?;
    if let Some(blocked) = view.blocked {
        let count = blocked.to_string();
        ui.add(
            dialog,
            small(Label::key_with("web.panel.blocked", &[("count", &count)])),
        )?;
    }
    let row = ui.add(dialog, Row::new(Label::key("web.panel.site")))?;
    let site = ui.add(row, Switch::new(Label::key("web.panel.site"), view.site_on))?;
    let mut rights = Vec::new();
    let options: Vec<Label> = Decision::ALL.iter().map(|d| Label::key(d.key())).collect();
    for (kind, decision) in &view.rights {
        let row = ui.add(dialog, Row::new(Label::key(kind.key())))?;
        let index = Decision::ALL
            .iter()
            .position(|d| d == decision)
            .unwrap_or(0);
        let select = Select::new(Label::key(kind.key()), options.clone()).selected(index);
        rights.push((ui.add(row, select)?, *kind));
    }
    let state = if view.secure {
        "web.panel.secure"
    } else {
        "web.panel.insecure"
    };
    let line = ui.add(dialog, Flex::row().cross(CrossAlign::Center).gap(Space::S4))?;
    let icon = if view.secure {
        IconId::Secure
    } else {
        IconId::Warning
    };
    ui.add(line, ultar_ui::widgets::Icon::new(icon).small())?;
    ui.add(line, small(Label::key(state)))?;
    let mut close = Button::new(Label::key("web.panel.close"));
    close.variant = ButtonVariant::Quiet;
    let close = ui.add(dialog, close)?;
    ui.set_cancel_action(dialog, Some(close))?;
    ui.open_panel(dialog, anchor)?;
    Ok(ShieldPanel {
        dialog,
        site,
        rights,
        close,
    })
}

/// The nodes of the download list.
#[derive(Debug, Clone)]
pub struct DownloadsPanel {
    /// The panel.
    pub dialog: NodeId,
    /// Open buttons.
    pub open: Vec<(NodeId, DownloadId)>,
    /// Show-in-folder buttons.
    pub show: Vec<(NodeId, DownloadId)>,
    /// Cancel buttons (running downloads).
    pub cancel: Vec<(NodeId, DownloadId)>,
    /// Clear.
    pub clear: Option<NodeId>,
}

fn status(d: &Download) -> Label {
    match d.state {
        State::Running { .. } => {
            let percent = d
                .share()
                .map_or_else(|| "…".to_string(), |s| format!("{:.0}%", s * 100.0));
            Label::key_with("web.dl.running", &[("percent", &percent)])
        }
        State::Done => Label::key("web.dl.done"),
        State::Failed => Label::key("web.dl.failed"),
        State::Cancelled => Label::key("web.dl.cancelled"),
    }
}

fn quiet(label: &str) -> Button {
    let mut b = Button::new(Label::key(label));
    b.variant = ButtonVariant::Quiet;
    b
}

/// Opens the download list at the downloads button.
pub fn open_downloads(
    ui: &mut Ui,
    anchor: NodeId,
    list: &[Download],
) -> Result<DownloadsPanel, UiError> {
    let dialog = panel(ui, Label::key("web.dl.title"))?;
    let mut p = DownloadsPanel {
        dialog,
        open: Vec::new(),
        show: Vec::new(),
        cancel: Vec::new(),
        clear: None,
    };
    if list.is_empty() {
        ui.add(
            dialog,
            EmptyState::new(IconId::Download, Label::key("web.dl.empty")),
        )?;
    }
    for d in list.iter().rev() {
        let item = ui.add(dialog, Flex::column().gap(Space::S4))?;
        ui.add(item, Text::new(Label::data(&d.name)))?;
        ui.add(item, small(status(d)))?;
        let buttons = ui.add(item, Flex::row().gap(Space::S8))?;
        match d.state {
            State::Running { .. } => p
                .cancel
                .push((ui.add(buttons, quiet("web.dl.cancel"))?, d.id)),
            State::Done => {
                p.open.push((ui.add(buttons, quiet("web.dl.open"))?, d.id));
                p.show.push((ui.add(buttons, quiet("web.dl.show"))?, d.id));
            }
            State::Failed | State::Cancelled => {}
        }
    }
    if list
        .iter()
        .any(|d| !matches!(d.state, State::Running { .. }))
    {
        p.clear = Some(ui.add(dialog, Button::new(Label::key("web.dl.clear")))?);
    }
    ui.open_panel(dialog, anchor)?;
    Ok(p)
}

/// The nodes of the settings dialog.
#[derive(Debug, Clone)]
pub struct SettingsDialog {
    /// The dialog.
    pub dialog: NodeId,
    /// Search engine choice.
    pub search: NodeId,
    /// Protection switch.
    pub protection: NodeId,
    /// Clear history.
    pub clear: NodeId,
    /// Close.
    pub close: NodeId,
}

/// Opens the settings.
pub fn open_settings(ui: &mut Ui, settings: &Settings) -> Result<SettingsDialog, UiError> {
    let root = ui.root();
    let dialog = ui.add(root, Dialog::new(Label::key("web.settings.title")))?;
    let names: Vec<Label> = SearchEngine::ALL
        .iter()
        .map(|e| Label::data(e.name()))
        .collect();
    let index = SearchEngine::ALL
        .iter()
        .position(|e| *e == settings.search)
        .unwrap_or(0);
    let row = ui.add(dialog, Row::new(Label::key("web.settings.search")))?;
    let search = ui.add(
        row,
        Select::new(Label::key("web.settings.search"), names).selected(index),
    )?;
    let row = ui.add(dialog, Row::new(Label::key("web.settings.protection")))?;
    let protection = ui.add(
        row,
        Switch::new(Label::key("web.settings.protection"), settings.protection),
    )?;
    let buttons = ui.add(dialog, Flex::row().gap(Space::S8))?;
    let clear = ui.add(buttons, Button::new(Label::key("web.settings.clear")))?;
    let mut close = Button::new(Label::key("web.settings.close"));
    close.variant = ButtonVariant::Primary;
    let close = ui.add(buttons, close)?;
    ui.set_cancel_action(dialog, Some(close))?;
    ui.set_default_action(dialog, Some(close))?;
    ui.open_dialog(dialog)?;
    Ok(SettingsDialog {
        dialog,
        search,
        protection,
        clear,
        close,
    })
}

/// The nodes of a permission prompt.
#[derive(Debug, Clone)]
pub struct PromptDialog {
    /// The dialog.
    pub dialog: NodeId,
    /// "Remember for this site".
    pub remember: NodeId,
    /// Allow.
    pub allow: NodeId,
    /// Block.
    pub block: NodeId,
}

/// Asks whether `host` may use `kind`. Block is the cancel action (Esc) and has the focus.
pub fn open_prompt(
    ui: &mut Ui,
    host: &str,
    kind: PermissionKind,
    private: bool,
) -> Result<PromptDialog, UiError> {
    let root = ui.root();
    let dialog = ui.add(root, Dialog::new(Label::key("web.prompt.title")))?;
    let what = ui.translator().t(kind.key()).into_owned();
    ui.add(
        dialog,
        Text::new(Label::key_with(
            "web.prompt.text",
            &[("host", host), ("what", &what)],
        )),
    )?;
    let remember = ui.add(
        dialog,
        Checkbox::new(Label::key("web.prompt.remember"), false),
    )?;
    if private {
        ui.set_disabled(remember, true)?;
    }
    let buttons = ui.add(dialog, Flex::row().gap(Space::S8))?;
    let block = ui.add(buttons, Button::new(Label::key("web.prompt.block")))?;
    let mut allow = Button::new(Label::key("web.prompt.allow"));
    allow.variant = ButtonVariant::Primary;
    let allow = ui.add(buttons, allow)?;
    ui.set_cancel_action(dialog, Some(block))?;
    ui.open_dialog(dialog)?;
    let _ = ui.focus(block);
    Ok(PromptDialog {
        dialog,
        remember,
        allow,
        block,
    })
}

/// Closes and removes a panel or dialog.
pub fn close(ui: &mut Ui, dialog: NodeId) {
    let _ = ui.close_dialog(dialog);
    let _ = ui.remove(dialog);
}
