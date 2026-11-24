//! Widget trees of the app, built from `ultar-ui` only (no own drawing, no fixed colours;
//! sizes from `design/tokens.toml` through [`metrics`]).
//!
//! The sidebar is built once; the content pane is rebuilt when the open page or its data
//! changes structurally, and updated in place otherwise (so focus stays where it is).

mod account;
mod dialog;
mod display;
mod display_rows;
mod drivers;
pub mod metrics;
mod panel;
mod shell;

use crate::action::{Act, Registry};
use crate::areas::PageKind;
use crate::state::{State, SubPage};
use ultar_i18n::Translator;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Card, Label, Text};

pub use account::strength as account_strength;
pub use dialog::open as open_dialog;
pub use display::{format_hz, inches};

/// Handles of the shown tree.
#[derive(Debug, Default)]
pub struct View {
    /// Node → action of the sidebar.
    pub shell: Registry,
    /// Node → action of the content pane and the dialog.
    pub page: Registry,
    /// The sidebar search field.
    pub search: Option<NodeId>,
    /// The navigation list.
    pub nav: Option<NodeId>,
    /// Area indices in the order the navigation list shows them.
    pub nav_areas: Vec<usize>,
    /// The content scroll container.
    pub main: Option<NodeId>,
    /// The content column inside it.
    pub content: Option<NodeId>,
    /// The open dialog and its fields.
    pub dialog: Option<NodeId>,
    /// Text fields of the open dialog in order.
    pub dialog_fields: Vec<NodeId>,
    /// Page-specific nodes updated in place (switches, values).
    pub live: Vec<(Act, NodeId)>,
}

impl View {
    /// The action of a node anywhere in the tree.
    pub fn act(&self, node: NodeId) -> Option<Act> {
        self.page.act(node).or_else(|| self.shell.act(node))
    }

    /// The node of an action anywhere in the tree.
    pub fn node(&self, act: Act) -> Option<NodeId> {
        self.page.node(act).or_else(|| self.shell.node(act))
    }

    /// Every (node, action) pair, sidebar first.
    pub fn entries(&self) -> Vec<(NodeId, Act)> {
        let mut all = self.shell.entries().to_vec();
        all.extend_from_slice(self.page.entries());
        all
    }
}

/// Builds the whole window.
pub fn build(ui: &mut Ui, state: &State, view: &mut View) -> Result<(), UiError> {
    ui.clear();
    *view = View::default();
    shell::build(ui, state, view)?;
    rebuild_content(ui, state, view)
}

/// Replaces the content pane with the page of the open area.
pub fn rebuild_content(ui: &mut Ui, state: &State, view: &mut View) -> Result<(), UiError> {
    if let Some(dialog) = view.dialog.take() {
        ui.close_dialog(dialog)?;
    }
    if let Some(old) = view.content.take() {
        ui.remove(old)?;
    }
    view.page = Registry::default();
    view.live.clear();
    view.dialog = None;
    view.dialog_fields.clear();
    let Some(main) = view.main else {
        return Ok(());
    };
    let column = Flex::column()
        .padding(Padding::symmetric(Space::S32, Space::S24))
        .gap(Space::S16);
    let content = ui.add(main, column)?;
    view.content = Some(content);

    let tr = ui.translator().clone();
    let cx = &mut Cx {
        ui,
        view,
        tr: &tr,
        state,
    };
    match (state.page(), state.sub) {
        (PageKind::Display, SubPage::NightLight) => panel::night_light(cx, content),
        (PageKind::Display, _) => display::build(cx, content),
        (PageKind::Drivers, _) => drivers::build(cx, content),
        (PageKind::Account, SubPage::CreateAccount) => account::create(cx, content),
        (PageKind::Account, _) => account::build(cx, content),
        (PageKind::Panel(id), _) => panel::generic(cx, content, id),
        (PageKind::NoService, _) => panel::no_service(cx, content),
    }
}

/// Rebuilds the navigation list after the search text changed.
pub fn refresh_nav(ui: &mut Ui, state: &State, view: &mut View) -> Result<(), UiError> {
    shell::refresh_nav(ui, state, view)
}

/// What the page builders share.
pub(crate) struct Cx<'a> {
    pub(crate) ui: &'a mut Ui,
    pub(crate) view: &'a mut View,
    pub(crate) tr: &'a Translator,
    pub(crate) state: &'a State,
}

impl Cx<'_> {
    /// Adds a clickable node and registers its action.
    pub(crate) fn act(
        &mut self,
        parent: NodeId,
        widget: impl Into<ultar_ui::widgets::Widget>,
        act: Act,
    ) -> Result<NodeId, UiError> {
        let id = self.ui.add(parent, widget)?;
        self.view.page.add(id, act);
        Ok(id)
    }

    /// Same as [`Cx::act`] with a sizing.
    pub(crate) fn act_sized(
        &mut self,
        parent: NodeId,
        widget: impl Into<ultar_ui::widgets::Widget>,
        sizing: Sizing,
        act: Act,
    ) -> Result<NodeId, UiError> {
        let id = self.ui.add_sized(parent, widget, sizing)?;
        self.view.page.add(id, act);
        Ok(id)
    }
}

/// Text specs of the boards.
pub(crate) mod spec {
    use super::*;

    /// Page title (h1, 24, bold, title family).
    pub(crate) fn page_title() -> TextSpec {
        TextSpec::title(TypeSize::S24)
    }

    /// Card title (h2, 16, semibold).
    pub(crate) fn card_title() -> TextSpec {
        TextSpec::new(TypeSize::S16).weight(Weight::Semibold)
    }

    /// Small explanatory text (12), wrapping.
    pub(crate) fn small() -> TextSpec {
        TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap)
    }

    /// Row label (14, medium).
    pub(crate) fn label() -> TextSpec {
        TextSpec::new(TypeSize::S14).weight(Weight::Medium)
    }
}

/// A page title; on compact surfaces with the back button before it (`settings.back`).
pub(crate) fn title(cx: &mut Cx<'_>, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
    if !shell::compact(cx.ui) {
        return cx.ui.add(parent, Text::new(label).spec(spec::page_title()));
    }
    let row = cx
        .ui
        .add(parent, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let back =
        ultar_ui::widgets::IconButton::new(ultar_ui::icon::IconId::Back, k("app.settings.back"));
    cx.act(row, back, Act::Back)?;
    cx.ui.add(row, Text::new(label).spec(spec::page_title()))
}

/// A small muted text.
pub(crate) fn muted(ui: &mut Ui, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
    let text = Text::new(label)
        .spec(spec::small())
        .color(ColorRole::TextMuted);
    ui.add(parent, text)
}

/// A card with a header (title, optional description); returns (card, header row).
pub(crate) fn section(
    ui: &mut Ui,
    parent: NodeId,
    sizing: Sizing,
    title: Label,
    desc: Option<Label>,
) -> Result<(NodeId, NodeId), UiError> {
    let card = ui.add_sized(parent, Card::new(), sizing)?;
    let header = ui.add(
        card,
        Flex::row()
            .padding(Padding::all(Space::S16))
            .gap(Space::S16)
            .cross(CrossAlign::Center),
    )?;
    let texts = ui.add_sized(header, Flex::column(), Sizing::grow(1))?;
    ui.add(texts, Text::new(title).spec(spec::card_title()))?;
    if let Some(desc) = desc {
        muted(ui, texts, desc)?;
    }
    Ok((card, header))
}

/// The label of a catalog key.
pub(crate) fn k(key: &str) -> Label {
    Label::key(key)
}
