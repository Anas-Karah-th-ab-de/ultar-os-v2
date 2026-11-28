//! Widget trees of the app, built from `ultar-ui` only (no own drawing, no fixed colours;
//! sizes from `design/tokens.toml` through [`metrics`]).
//!
//! The sidebar is built once; the content pane is rebuilt when the open page or its data
//! changes structurally, and updated in place otherwise (so focus stays where it is).

mod about;
mod account;
pub mod bluetooth;
mod cloud;
mod dialog;
mod display;
mod display_rows;
mod drivers;
pub mod foreign;
pub mod helper;
pub mod mesh;
pub mod metrics;
mod more;
pub(crate) mod panel;
mod shell;
pub mod status;
mod voice;

use crate::action::{Act, Registry};
use crate::areas::PageKind;
use crate::state::{State, SubPage};
use ultar_i18n::Translator;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{BarEdge, Card, Label, Text};

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
    // Boards: wide pages 24 / 32 padded; compact pages (phone) 16 at the sides, the page
    // starts right under the status bar.
    let column = if shell::compact(ui) {
        Flex::column().padding(compact_padding()).gap(Space::S16)
    } else {
        Flex::column()
            .padding(Padding::symmetric(Space::S32, Space::S24))
            .gap(Space::S16)
    };
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
        (PageKind::Panel(id), _) => {
            panel::generic(cx, content, id)?;
            if id == "assistant" {
                helper::build(cx, content)?;
                cloud::build(cx, content)?;
                voice::build(cx, content)?;
            }
            Ok(())
        }
        (PageKind::NoService, _) => panel::no_service(cx, content),
        (PageKind::Status(kind), _) => status::build(cx, content, kind),
        (PageKind::Foreign, SubPage::ForeignApp) => foreign::app(cx, content),
        (PageKind::Foreign, _) => foreign::build(cx, content),
        (PageKind::Mesh, SubPage::MeshDevice) => mesh::device(cx, content),
        (PageKind::Mesh, _) => mesh::build(cx, content),
        (PageKind::About, _) => about::build(cx, content),
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

    /// Page title (h1, 24, bold, title family); like every title it wraps between words when
    /// the page is narrower than the text (never cut off).
    pub(crate) fn page_title() -> TextSpec {
        TextSpec::title(TypeSize::S24).overflow(Overflow::Wrap)
    }

    /// Card title (h2: title family, 16, semibold).
    pub(crate) fn card_title() -> TextSpec {
        TextSpec::title(TypeSize::S16)
            .weight(Weight::Semibold)
            .overflow(Overflow::Wrap)
    }

    /// Small explanatory text (12), wrapping.
    pub(crate) fn small() -> TextSpec {
        TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap)
    }

    /// Row label (14, medium).
    pub(crate) fn label() -> TextSpec {
        TextSpec::new(TypeSize::S14)
            .weight(Weight::Medium)
            .overflow(Overflow::Wrap)
    }
}

/// A page title; on compact surfaces with the back button before it (`settings.back`).
pub(crate) fn title(cx: &mut Cx<'_>, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
    if !shell::compact(cx.ui) {
        return cx.ui.add(parent, Text::new(label).spec(spec::page_title()));
    }
    // Boards (`apptitle`): a touch-high row, the back button a touch target.
    let touch = ultar_ui::tokens::size::TARGET_TOUCH_MIN;
    let row = cx.ui.add_sized(
        parent,
        Flex::row().gap(Space::S8).cross(CrossAlign::Center),
        Sizing::AUTO.with_height(touch),
    )?;
    let back =
        ultar_ui::widgets::IconButton::new(ultar_ui::icon::IconId::Back, k("app.settings.back"));
    cx.act_sized(
        row,
        back,
        Sizing::width(touch).with_height(touch),
        Act::Back,
    )?;
    cx.ui.add(row, Text::new(label).spec(spec::page_title()))
}

/// A small muted text.
pub(crate) fn muted(ui: &mut Ui, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
    let text = Text::new(label)
        .spec(spec::small())
        .color(ColorRole::TextMuted);
    ui.add(parent, text)
}

/// Narrowest page on which a card header keeps its buttons beside the title (the board's
/// main column); below it the header wraps.
const HEADER_ROW_MIN: f32 = 640.0;

/// `true` on a page too narrow for texts and controls side by side in one header or row.
pub(crate) fn narrow(ui: &Ui) -> bool {
    content_width(ui) < HEADER_ROW_MIN
}

/// A header or row of texts and controls: one row, or on a narrow page a wrapping row (the
/// controls continue on the next line instead of squeezing the texts).
pub(crate) fn reflow(row: Flex, narrow: bool) -> Flex {
    if narrow {
        row.wrap()
    } else {
        row
    }
}

/// The size of the text column of such a row: the space the controls leave, or on a narrow
/// page its natural size (a wrapping row keeps natural sizes).
pub(crate) fn beside(narrow: bool) -> Sizing {
    if narrow {
        Sizing::AUTO
    } else {
        Sizing::grow(1)
    }
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
    // Boards (`cardhead`): padding 16, a hairline under it, the title in the title family.
    // On a narrow page the header wraps: the texts take the whole line, the header's buttons
    // (added by the caller) follow below them instead of squeezing title and description.
    let narrow = narrow(ui);
    let row = Flex::row()
        .padding(Padding::all(Space::S16))
        .gap(Space::S16)
        .cross(CrossAlign::Center);
    let header = ui.add(card, Card::rule(BarEdge::Bottom).flex(reflow(row, narrow)))?;
    let texts_sizing = beside(narrow);
    let texts = ui.add_sized(header, Flex::column(), texts_sizing)?;
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

/// The exact height of `lines` lines of `size` text (CSS line boxes of the boards are not
/// rounded; rounding every text would let long tables drift from the board).
pub(crate) fn lines_height(ui: &Ui, size: TypeSize, lines: f32) -> f32 {
    let factor = if ui.env().lang == ultar_i18n::Lang::Ar {
        ultar_ui::tokens::typography::LINE_HEIGHT_ARABIC
    } else {
        ultar_ui::tokens::typography::LINE_HEIGHT_LATIN
    };
    size.points() * factor * lines
}

/// Padding of a compact page: 16 at the sides and at the bottom, none at the top (boards:
/// the phone screen's own padding lies under the status bar).
/// The width a page's content column has: the window less the sidebar and the page's padding at
/// the sides. Pages lay out their columns for it (side by side only when they fit, else stacked).
pub(crate) fn content_width(ui: &Ui) -> f32 {
    let w = ui.size().w;
    if shell::compact(ui) {
        (w - 2.0 * Space::S16.points()).max(0.0)
    } else {
        (w - metrics::sidebar() - 2.0 * Space::S32.points()).max(0.0)
    }
}

pub(crate) fn compact_padding() -> Padding {
    Padding {
        top: None,
        ..Padding::symmetric(Space::S16, Space::S12)
    }
}
