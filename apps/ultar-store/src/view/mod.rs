//! The screens (APP-RULES §2 `view/`): every page and every state, built with
//! `ultar-ui` widgets only. No colour, size or radius is written here: every
//! value is a token; every visible text is a catalog key (`Label::key`) or
//! catalogue data (`Label::data`).
//!
//! The window width chooses the layout: the wide layout is the board "Store",
//! the compact one the boards "CompactStore"/"CompactStoreApp" (tokens
//! `layout.compact_max`). The shared page states come from "SurfaceStates".
//!
//! [`build`] rebuilds the whole tree for the current [`Store`] and fills a
//! [`HitMap`]: every clickable node is bound to exactly one [`StoreAction`],
//! so there are no dead clicks, and the click tests iterate the map to prove
//! every `store.*` action of docs/ui/CLICKS.md does what it says.

use crate::action::StoreAction;
use crate::catalog::Loc;
use crate::state::{OpState, Store};
use std::collections::BTreeMap;
use ultar_ui::event::{Action, ActionKind};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Label, Menu, MenuItem, Text, Widget};

mod app_page;
mod chrome;
mod discover;
mod lists;
mod rows;
mod states;

/// The desktop nav column width (board Store: 240).
pub(crate) const NAV_WIDTH: f32 = 240.0;

/// Which action each clickable node performs (rebuilt every frame).
#[derive(Default)]
pub struct HitMap {
    hits: BTreeMap<NodeId, StoreAction>,
    search: Option<NodeId>,
    tabs: Vec<(NodeId, Vec<crate::state::Area>)>,
    menus: Vec<(NodeId, Vec<crate::state::Area>)>,
}

impl HitMap {
    /// The search field node (its text is read after `TextChanged`).
    pub fn search_field(&self) -> Option<NodeId> {
        self.search
    }

    /// The action bound to this node, if it is clickable.
    pub fn action_of(&self, node: NodeId) -> Option<&StoreAction> {
        self.hits.get(&node)
    }

    /// Every distinct action id reachable on the current screen.
    pub fn ids(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = self.hits.values().map(StoreAction::id).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub(crate) fn bind(&mut self, node: NodeId, action: StoreAction) {
        self.hits.insert(node, action);
    }

    pub(crate) fn set_search(&mut self, node: NodeId) {
        self.search = Some(node);
    }

    pub(crate) fn set_tabs(&mut self, node: NodeId, areas: Vec<crate::state::Area>) {
        self.tabs.push((node, areas));
    }

    pub(crate) fn set_menu(&mut self, owner: NodeId, areas: Vec<crate::state::Area>) {
        self.menus.push((owner, areas));
    }

    /// Translates one emitted toolkit action into a `StoreAction`.
    pub fn translate(&self, ui: &Ui, action: &Action) -> Option<StoreAction> {
        match &action.kind {
            ActionKind::TextChanged if self.search == Some(action.node) => {
                Some(StoreAction::Search(ui_text(ui, action.node)))
            }
            ActionKind::Selected(index) => {
                if let Some((_, areas)) = self.tabs.iter().find(|(n, _)| *n == action.node) {
                    return areas.get(*index).copied().map(StoreAction::Area);
                }
                self.action_of(action.node).cloned()
            }
            ActionKind::MenuChosen(id) => {
                let index = usize::try_from(*id).ok().and_then(|i| i.checked_sub(1));
                let areas = self.menus.first().map(|(_, a)| a);
                match (areas, index) {
                    (Some(areas), Some(i)) => areas.get(i).copied().map(StoreAction::Area),
                    _ => None,
                }
            }
            _ => self.action_of(action.node).cloned(),
        }
    }

    /// Opens the `store.more` menu of an owner node (after a `StoreAction::More`).
    pub fn open_more(&self, ui: &mut Ui, owner: NodeId) -> Result<(), UiError> {
        let areas = self.menus.iter().find(|(n, _)| *n == owner).map(|(_, a)| a.clone());
        match areas {
            Some(areas) => {
                let items: Vec<MenuItem> = areas
                    .iter()
                    .enumerate()
                    .map(|(i, area)| {
                        MenuItem::new(
                            u32::try_from(i + 1).unwrap_or(u32::MAX),
                            Label::key(chrome::area_key(*area)),
                        )
                        .icon(chrome::area_icon(*area))
                    })
                    .collect();
                ui.open_menu(owner, Menu::new(items))
            }
            None => Ok(()),
        }
    }
}

fn ui_text(ui: &Ui, node: NodeId) -> String {
    ui.text(node).unwrap_or_default().to_string()
}

/// Adds a widget and binds it to one action.
pub(crate) fn bind_add(
    ui: &mut Ui,
    hits: &mut HitMap,
    parent: NodeId,
    widget: impl Into<Widget>,
    action: StoreAction,
) -> Result<NodeId, UiError> {
    let node = ui.add(parent, widget)?;
    hits.bind(node, action);
    Ok(node)
}

/// Builds the full screen for `store` into `ui`; fills `hits`.
pub fn build(ui: &mut Ui, store: &Store, hits: &mut HitMap) -> Result<(), UiError> {
    *hits = HitMap::default();
    ui.clear();
    ui.set_background(ColorRole::Ground);
    if is_compact(ui) {
        chrome::compact_root(ui, ui.root(), store, hits)
    } else {
        chrome::wide_root(ui, ui.root(), store, hits)
    }
}

/// A block container (column) added to `parent`.
pub(crate) fn column(
    ui: &mut Ui,
    parent: NodeId,
    gap: Space,
    sizing: Sizing,
) -> Result<NodeId, UiError> {
    ui.add_sized(parent, Flex::column().gap(gap), sizing)
}

/// A spacer that soaks the remaining main-axis width in a row.
pub(crate) fn spacer(ui: &mut Ui, parent: NodeId) -> Result<NodeId, UiError> {
    ui.add_sized(parent, Flex::row(), Sizing::grow(1))
}

/// The catalogue `Loc` text as a data label in the UI language.
pub(crate) fn loc_label(loc: &Loc, ui: &Ui) -> Label {
    Label::data(loc.text(ui.env().lang))
}

/// The name of one catalogue app (data), or the id when the catalogue lacks it.
pub(crate) fn app_name(store: &Store, id: &str, ui: &Ui) -> Label {
    match store.entry(id) {
        Some(app) => loc_label(&app.name, ui),
        None => Label::data(id),
    }
}

/// Whether the compact (phone) layout is active.
pub(crate) fn is_compact(ui: &Ui) -> bool {
    ui.size_class() == ultar_ui::tokens::SizeClass::Compact
}

/// The em-dash every board uses for "value comes at run time".
pub(crate) const DASH: &str = "—";

/// A byte count formatted for the current language, or `—` when unknown
/// (the boards' footnote: sizes come from the catalogue at run time).
pub(crate) fn size_text(bytes: Option<u64>, ui: &Ui) -> String {
    match bytes {
        Some(b) => ultar_i18n::units::format_size(b, ui.translator()),
        None => DASH.to_string(),
    }
}

/// The catalogue icon name mapped to the system icon set.
pub(crate) fn icon_by_name(name: &str) -> IconId {
    IconId::ALL
        .iter()
        .copied()
        .find(|i| i.name() == name)
        .unwrap_or(IconId::Package)
}

/// A muted secondary line.
pub(crate) fn muted(
    ui: &mut Ui,
    parent: NodeId,
    label: Label,
    size: TypeSize,
) -> Result<NodeId, UiError> {
    ui.add(
        parent,
        Text::new(label)
            .spec(TextSpec::new(size))
            .color(ColorRole::TextMuted),
    )
}

/// The kind of the main button on an app page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainAction {
    Install,
    Update,
    Open,
    /// The catalogue does not carry this app (opened from the Installed list).
    InstalledOnly,
}

pub(crate) fn main_action(store: &Store, app: &str) -> MainAction {
    let known = store.entry(app).is_some();
    if store.is_installed(app) {
        if known && store.update_available(app) {
            MainAction::Update
        } else {
            MainAction::Open
        }
    } else if known {
        MainAction::Install
    } else {
        MainAction::InstalledOnly
    }
}

/// The percent of the running operation of this app (`Some(None)` = no number).
pub(crate) fn running_progress(store: &Store, app: &str) -> Option<Option<f32>> {
    match store.operation.as_ref().and_then(|o| o.state(app)) {
        Some(OpState::Running(percent)) => Some(*percent),
        _ => None,
    }
}

/// True when the no-space surface is open (it replaces the app page content).
pub(crate) fn no_space_open(store: &Store) -> bool {
    store
        .operation
        .as_ref()
        .is_some_and(|o| o.no_space_for.is_some())
}

pub(crate) fn no_space_app(store: &Store) -> Option<&str> {
    store
        .operation
        .as_ref()
        .and_then(|o| o.no_space_for.as_deref())
}
