//! The browser window drawn with `ultar-ui`, exactly as the board "DesktopBottom" draws the
//! Ultar Browser (design/boards/project/DesktopBottom.dc.html; the title bar with the
//! window buttons is the compositor's): a tab strip (48, `raised`, closable tabs, new-tab
//! button), a toolbar (48: back, forward, reload, the address field, tracking protection,
//! downloads) and the page or the start page. Panels, the menu and dialogs: [`panels`].
//!
//! [`Chrome::update`] rebuilds the tree only when its structure changes (tabs, the shown
//! tab, start page or page, protection state) and otherwise sets states in place, so typing
//! and focus are never disturbed by page events.

pub mod panels;

use crate::engine::TabId;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Length, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    BarEdge, Button, ButtonVariant, Card, Icon, IconButton, Label, PixelArea, SearchField, Text,
};

/// Height of the tab strip and of the toolbar: 48 plus the 1-point line under each (the
/// boards' CSS boxes are content-box).
pub const STRIP_HEIGHT: f32 = 49.0;
/// Width of the start page's search field (boards).
pub const START_FIELD_WIDTH: f32 = 480.0;
/// Longest tab label in characters.
pub const MAX_TAB_LABEL: usize = 24;

/// A clickable element of the window and its action of docs/ui/CLICKS.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    /// Shows tab n (`browser.tab`).
    Tab(usize),
    /// Closes tab n (`browser.tabclose`).
    TabClose(usize),
    /// New tab (`browser.newtab`).
    NewTab,
    /// Back (`browser.back`).
    Back,
    /// Forward (`browser.forward`).
    Forward,
    /// Reload (`browser.reload`); while loading the same place stops the load.
    Reload,
    /// The address field (`browser.address`).
    Address,
    /// The start page's search field (`browser.address`).
    StartSearch,
    /// Tracking protection (`browser.shield`).
    Shield,
    /// Downloads (`browser.downloads`).
    Downloads,
    /// The page itself.
    Page,
}

impl Act {
    /// The action name of docs/ui/CLICKS.md.
    pub fn name(self) -> &'static str {
        match self {
            Act::Tab(_) => "browser.tab",
            Act::TabClose(_) => "browser.tabclose",
            Act::NewTab => "browser.newtab",
            Act::Back => "browser.back",
            Act::Forward => "browser.forward",
            Act::Reload => "browser.reload",
            Act::Address | Act::StartSearch => "browser.address",
            Act::Shield => "browser.shield",
            Act::Downloads => "browser.downloads",
            Act::Page => "browser.page",
        }
    }
}

/// One tab as the strip shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabView {
    /// Title or host; `None` shows "New tab" ("Private tab" for a private one).
    pub title: Option<String>,
    /// Private tab.
    pub private: bool,
}

/// What the window shows.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The tabs.
    pub tabs: Vec<TabView>,
    /// The shown tab.
    pub active: usize,
    /// History behind.
    pub can_back: bool,
    /// History ahead.
    pub can_forward: bool,
    /// Loading.
    pub loading: bool,
    /// Address of the shown tab (empty on the start page).
    pub address: String,
    /// Tracking protection applies to the shown page.
    pub protection: bool,
    /// The shown tab is on the start page.
    pub start_page: bool,
    /// The engine tab shown (the page picture belongs to it).
    pub page: Option<TabId>,
}

#[derive(Debug, Clone, PartialEq)]
struct Structure {
    tabs: Vec<bool>,
    active: usize,
    start_page: bool,
    protection: bool,
}

impl Structure {
    fn of(view: &View) -> Structure {
        Structure {
            tabs: view.tabs.iter().map(|t| t.private).collect(),
            active: view.active,
            start_page: view.start_page,
            protection: view.protection,
        }
    }
}

/// The nodes of the window.
#[derive(Debug, Clone)]
pub struct Chrome {
    /// Tab strip region.
    pub strip: NodeId,
    /// Toolbar region.
    pub toolbar: NodeId,
    /// (tab button, close button) per tab, in strip order.
    pub tabs: Vec<(NodeId, NodeId)>,
    /// The chip around each tab.
    pub chips: Vec<NodeId>,
    /// New tab button.
    pub new_tab: NodeId,
    /// Back.
    pub back: NodeId,
    /// Forward.
    pub forward: NodeId,
    /// Reload or stop.
    pub reload: NodeId,
    /// Address field.
    pub address: NodeId,
    /// Tracking protection button.
    pub shield: NodeId,
    /// Downloads button.
    pub downloads: NodeId,
    /// The page picture (when a page is shown).
    pub page: Option<NodeId>,
    /// The start page's search field (on the start page).
    pub start_search: Option<NodeId>,
    structure: Structure,
    loading: bool,
}

fn tab_label(view: &TabView) -> Label {
    match &view.title {
        Some(text) if text.chars().count() > MAX_TAB_LABEL => {
            let cut: String = text.chars().take(MAX_TAB_LABEL - 1).collect();
            Label::data(&format!("{cut}…"))
        }
        Some(text) => Label::data(text),
        None if view.private => Label::key("web.tab.private"),
        None => Label::key("web.tab.new"),
    }
}

/// Padding at the start and end and 8 at the top (the 32-point controls sit 8 below the
/// top of a 48-point bar).
fn inline(space: Space) -> Padding {
    Padding {
        start: Some(space),
        end: Some(space),
        top: Some(Space::S8),
        bottom: None,
    }
}

fn close_name(label: &Label, ui: &Ui) -> Label {
    let name = label.resolve(ui.translator()).into_owned();
    Label::key_with("web.tab.close", &[("name", &name)])
}

fn strip_sizing() -> Sizing {
    Sizing {
        width: Length::Auto,
        height: Length::Points(STRIP_HEIGHT),
        grow: 0,
    }
}

impl Chrome {
    /// Builds the window for `view` into an empty `ui`.
    pub fn build(ui: &mut Ui, view: &View) -> Result<Chrome, UiError> {
        ui.clear();
        ui.set_background(ColorRole::Surface);
        ui.set_root_flex(Flex::column());
        let root = ui.root();
        let strip_flex = Flex::row()
            .cross(CrossAlign::Start)
            .gap(Space::S4)
            .padding(inline(Space::S8));
        let strip = ui.add_sized(
            root,
            Card::region(ColorRole::Raised, BarEdge::Bottom).flex(strip_flex),
            strip_sizing(),
        )?;
        let (chips, tabs) = Chrome::add_tabs(ui, strip, view)?;
        let new_tab = ui.add(
            strip,
            IconButton::new(IconId::Plus, Label::key("web.tab.new")),
        )?;
        let mut chrome = Chrome::add_toolbar(ui, view)?;
        chrome.strip = strip;
        chrome.chips = chips;
        chrome.tabs = tabs;
        chrome.new_tab = new_tab;
        chrome.add_content(ui, view)?;
        chrome.sync(ui, view)?;
        Ok(chrome)
    }

    /// The toolbar; the strip's nodes are filled in by [`Chrome::build`].
    fn add_toolbar(ui: &mut Ui, view: &View) -> Result<Chrome, UiError> {
        let root = ui.root();
        let bar_flex = Flex::row()
            .cross(CrossAlign::Start)
            .gap(Space::S8)
            .padding(inline(Space::S12));
        let toolbar = ui.add_sized(
            root,
            Card::region(ColorRole::Surface, BarEdge::Bottom).flex(bar_flex),
            strip_sizing(),
        )?;
        let icon = |icon, key| IconButton::new(icon, Label::key(key));
        let back = ui.add(toolbar, icon(IconId::Back, "web.back"))?;
        let forward = ui.add(toolbar, icon(IconId::Forward, "web.forward"))?;
        let reload = ui.add(toolbar, icon(IconId::Reload, "web.reload"))?;
        let field =
            SearchField::new(Label::key("web.address")).placeholder(Label::key("web.address"));
        let address = ui.add_sized(toolbar, field.max_len(crate::url::MAX_URL), Sizing::grow(1))?;
        let shield = ui.add(toolbar, Chrome::shield_button(view.protection))?;
        let downloads = ui.add(toolbar, icon(IconId::Download, "web.downloads"))?;
        Ok(Chrome {
            strip: toolbar,
            toolbar,
            tabs: Vec::new(),
            chips: Vec::new(),
            new_tab: toolbar,
            back,
            forward,
            reload,
            address,
            shield,
            downloads,
            page: None,
            start_search: None,
            structure: Structure::of(view),
            loading: false,
        })
    }

    fn shield_button(on: bool) -> IconButton {
        let (key, color) = if on {
            ("web.shield.on", ColorRole::Success)
        } else {
            ("web.shield.off", ColorRole::TextMuted)
        };
        let second = if on { IconId::Check } else { IconId::Close };
        IconButton::new(IconId::Block, Label::key(key)).status(second, color)
    }

    #[allow(clippy::type_complexity)]
    fn add_tabs(
        ui: &mut Ui,
        strip: NodeId,
        view: &View,
    ) -> Result<(Vec<NodeId>, Vec<(NodeId, NodeId)>), UiError> {
        let (mut chips, mut tabs) = (Vec::new(), Vec::new());
        for (i, tab) in view.tabs.iter().enumerate() {
            let shown = i == view.active;
            let chip = ui.add(
                strip,
                Card::chip(shown).flex(Flex::row().cross(CrossAlign::Center)),
            )?;
            let label = tab_label(tab);
            let mut button = Button::new(label.clone());
            button.variant = ButtonVariant::Tab(shown);
            if tab.private {
                button = button.icon(IconId::Eye);
            }
            let tab_button = ui.add(chip, button)?;
            let close = ui.add(chip, IconButton::new(IconId::Close, close_name(&label, ui)))?;
            // The chip's 1-point border after the close button (boards).
            ui.add_sized(
                chip,
                Flex::row(),
                Sizing::width(ultar_ui::tokens::size::BORDER),
            )?;
            chips.push(chip);
            tabs.push((tab_button, close));
        }
        Ok((chips, tabs))
    }

    fn add_content(&mut self, ui: &mut Ui, view: &View) -> Result<(), UiError> {
        let root = ui.root();
        if !view.start_page {
            self.page = Some(ui.add_sized(
                root,
                PixelArea::new(Label::key("web.page")),
                Sizing::grow(1),
            )?);
            return Ok(());
        }
        let page_flex = Flex::column()
            .main(MainAlign::Center)
            .cross(CrossAlign::Center)
            .gap(Space::S24);
        let start = ui.add_sized(
            root,
            Card::region(ColorRole::Surface, BarEdge::Top).flex(page_flex),
            Sizing::grow(1),
        )?;
        let head = ui.add(start, Flex::row().cross(CrossAlign::Center).gap(Space::S12))?;
        ui.add(head, Icon::new(IconId::Mark).color(ColorRole::AccentText))?;
        let title = TextSpec::title(TypeSize::S24).weight(Weight::Bold);
        ui.add(head, Text::new(Label::key("web.title")).spec(title))?;
        let field = SearchField::new(Label::key("web.start.search"))
            .placeholder(Label::key("web.start.search"));
        let search = ui.add_sized(
            start,
            field.max_len(crate::url::MAX_URL),
            Sizing::width(START_FIELD_WIDTH),
        )?;
        let note = ui.add(start, Flex::row().cross(CrossAlign::Center).gap(Space::S4))?;
        ui.add(
            note,
            Icon::new(IconId::Block).small().color(ColorRole::TextMuted),
        )?;
        let key = if view.tabs.get(view.active).is_some_and(|t| t.private) {
            "web.start.private"
        } else {
            "web.start.note"
        };
        let small = TextSpec::new(TypeSize::S12);
        ui.add(
            note,
            Text::new(Label::key(key))
                .spec(small)
                .color(ColorRole::TextMuted),
        )?;
        self.start_search = Some(search);
        Ok(())
    }

    /// Brings the window to `view`: rebuilds when the structure changed, else sets states.
    /// Returns `true` after a rebuild (open panels were closed by it).
    pub fn update(&mut self, ui: &mut Ui, view: &View) -> Result<bool, UiError> {
        if Structure::of(view) != self.structure {
            let focus = ui.focused().and_then(|f| self.act(f));
            *self = Chrome::build(ui, view)?;
            if let Some(node) = focus.and_then(|a| self.node(a)) {
                let _ = ui.focus(node);
            }
            return Ok(true);
        }
        self.sync(ui, view)?;
        Ok(false)
    }

    fn sync(&mut self, ui: &mut Ui, view: &View) -> Result<(), UiError> {
        for (i, tab) in view.tabs.iter().enumerate() {
            if let Some(&(button, close)) = self.tabs.get(i) {
                let label = tab_label(tab);
                ui.set_label(button, label.clone())?;
                let name = close_name(&label, ui);
                ui.set_accessible_name(close, Some(name))?;
            }
        }
        ui.set_disabled(self.back, !view.can_back)?;
        ui.set_disabled(self.forward, !view.can_forward)?;
        if ui.focused() != Some(self.address) && ui.text(self.address)? != view.address {
            ui.set_text(self.address, &view.address)?;
        }
        if view.loading != self.loading {
            self.swap_reload(ui, view.loading)?;
        }
        Ok(())
    }

    /// Reload and stop share one place: while loading the button is "Stop loading".
    fn swap_reload(&mut self, ui: &mut Ui, loading: bool) -> Result<(), UiError> {
        let focused = ui.focused() == Some(self.reload);
        ui.remove(self.reload)?;
        let button = if loading {
            IconButton::new(IconId::Close, Label::key("web.stop"))
        } else {
            IconButton::new(IconId::Reload, Label::key("web.reload"))
        };
        self.reload = ui.add(self.toolbar, button)?;
        let last = ui.children(self.toolbar).len().saturating_sub(1);
        ui.move_child(self.toolbar, last, 2)?;
        self.loading = loading;
        if focused {
            let _ = ui.focus(self.reload);
        }
        Ok(())
    }

    /// `true` while the reload place is the stop button.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// The element a node is.
    pub fn act(&self, node: NodeId) -> Option<Act> {
        if let Some(i) = self.tabs.iter().position(|t| t.0 == node) {
            return Some(Act::Tab(i));
        }
        if let Some(i) = self.tabs.iter().position(|t| t.1 == node) {
            return Some(Act::TabClose(i));
        }
        let fixed = [
            (self.new_tab, Act::NewTab),
            (self.back, Act::Back),
            (self.forward, Act::Forward),
            (self.reload, Act::Reload),
            (self.address, Act::Address),
            (self.shield, Act::Shield),
            (self.downloads, Act::Downloads),
        ];
        if let Some((_, act)) = fixed.iter().find(|(n, _)| *n == node) {
            return Some(*act);
        }
        if Some(node) == self.start_search {
            return Some(Act::StartSearch);
        }
        (Some(node) == self.page).then_some(Act::Page)
    }

    /// The node of an element.
    pub fn node(&self, act: Act) -> Option<NodeId> {
        Some(match act {
            Act::Tab(i) => self.tabs.get(i)?.0,
            Act::TabClose(i) => self.tabs.get(i)?.1,
            Act::NewTab => self.new_tab,
            Act::Back => self.back,
            Act::Forward => self.forward,
            Act::Reload => self.reload,
            Act::Address => self.address,
            Act::StartSearch => self.start_search?,
            Act::Shield => self.shield,
            Act::Downloads => self.downloads,
            Act::Page => self.page?,
        })
    }
}
