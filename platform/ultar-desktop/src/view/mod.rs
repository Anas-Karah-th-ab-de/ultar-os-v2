//! Widget trees of the desktop, built from `ultar-ui` bricks and widgets only, with the
//! geometry of the boards (`design/boards/gen.py`: `bottombar`, `sheet`, `qt`, `notif`,
//! the overview and the projection panel). Every operable node is registered with a
//! [`Target`]: what it means for the shell.

pub mod apps;
pub mod banners;
pub mod bar;
pub mod calendar;
pub mod geometry;
pub mod menu;
pub mod note;
pub mod overview;
pub mod projection;
pub mod quick;

use crate::model::quick::{PowerMode, TileId};
use crate::model::search::Hit;
use geometry::Placement;
use ultar_i18n::{Lang, Translator};
use ultar_ui::geom::Size;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, Space, ThemeKind};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::Card;

/// What an operable node means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Bar: trash.
    Trash,
    /// Bar: overview.
    Overview,
    /// Bar: all apps.
    AllApps,
    /// Bar: search (opens the overview search).
    Search,
    /// Bar: the tile of an app (index of `Desktop::bar_apps`).
    App(usize),
    /// Bar: input language.
    Keyboard,
    /// Bar: status (quick settings).
    Status,
    /// Bar: clock (calendar and notifications).
    Clock,
    /// Quick settings: the switch part of a tile.
    Tile(TileId),
    /// Quick settings: the "choose" part of a tile.
    TileMore(TileId),
    /// Quick settings: back to the tiles.
    QuickBack,
    /// Quick settings: one of the power modes.
    PowerModeChoice(PowerMode),
    /// Quick settings: volume slider.
    Volume,
    /// Quick settings: output device.
    Output,
    /// Quick settings: brightness slider.
    Brightness,
    /// Quick settings: battery (Settings > Power).
    Battery,
    /// Quick settings: screenshot.
    Screenshot,
    /// Quick settings: Settings.
    Settings,
    /// Quick settings: lock.
    Lock,
    /// Quick settings, all apps: power menu.
    Power,
    /// Calendar: do not disturb.
    Dnd,
    /// Calendar: clear all notifications.
    ClearAll,
    /// Calendar: undo "clear all".
    Undo,
    /// Calendar: dismiss one notification.
    Dismiss(u64),
    /// Calendar: previous month.
    PrevMonth,
    /// Calendar: next month.
    NextMonth,
    /// Calendar: the month grid.
    Days,
    /// All apps / overview: the search field.
    SearchField,
    /// All apps: an app of the list.
    AppItem(String),
    /// The category tabs of the app grid.
    Categories,
    /// Overview: a search result.
    Result(Hit),
    /// Overview: a workspace (the compositor has one).
    Workspace(u32),
    /// Overview: a window preview.
    Window(u32),
    /// Overview: close a window.
    CloseWindow(u32),
    /// Display mode panel: mode 0–3.
    Mode(usize),
    /// A menu surface: its anchor (items arrive as `MenuChosen`).
    Menu,
    /// Lock: show the sign-in.
    Unlock,
    /// Lock: emergency.
    Emergency,
    /// Lock: emergency power-off.
    EmergencyOff,
    /// Lock, dialogs: the password field.
    Password,
    /// Lock/login: a user card (index into the users).
    User(usize),
    /// Lock: password ↔ PIN.
    Method,
    /// Lock: sign in.
    Submit,
    /// Lock: accessibility menu.
    Access,
    /// Lock: back to the cover.
    Back,
    /// Dialogs: allow once.
    AllowOnce,
    /// Dialogs: allow always.
    AllowAlways,
    /// Dialogs: deny / cancel / revert.
    Deny,
    /// Dialogs: confirm (authenticate, keep).
    Confirm,
    /// Dialogs: more time for "keep?".
    MoreTime,
}

/// A built tree and what its nodes mean.
pub struct View {
    /// The tree.
    pub ui: Ui,
    /// Operable nodes.
    pub targets: Vec<(NodeId, Target)>,
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View")
            .field("targets", &self.targets.len())
            .finish()
    }
}

impl View {
    /// A view around a tree.
    pub fn new(ui: Ui) -> View {
        View {
            ui,
            targets: Vec::new(),
        }
    }

    /// Registers what a node means; returns the node.
    pub fn mark(&mut self, id: NodeId, target: Target) -> NodeId {
        self.targets.push((id, target));
        id
    }

    /// What a node means.
    pub fn target(&self, id: NodeId) -> Option<&Target> {
        self.targets.iter().find(|(n, _)| *n == id).map(|(_, t)| t)
    }

    /// The node of a target.
    pub fn node(&self, target: &Target) -> Option<NodeId> {
        self.targets
            .iter()
            .find(|(_, t)| t == target)
            .map(|(n, _)| *n)
    }
}

/// Look and language of a tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Language (direction follows).
    pub lang: Lang,
    /// Theme.
    pub theme: ThemeKind,
    /// Device pixels per point.
    pub scale: f32,
    /// Pointer or touch targets.
    pub mode: InputMode,
}

impl Look {
    /// Pointer input at scale 1.
    pub fn new(lang: Lang, theme: ThemeKind) -> Look {
        Look {
            lang,
            theme,
            scale: 1.0,
            mode: InputMode::Pointer,
        }
    }
}

/// An empty tree of `size` in a look.
pub fn new_ui(look: Look, size: Size, tr: &Translator) -> Result<Ui, UiError> {
    let mut env = PaintEnv::new(look.lang, look.theme);
    env.scale = look.scale;
    Ui::new(env, look.mode, size, tr.clone())
}

/// How a floating card is placed on the screen.
pub type PlaceFn = fn(Size, Size, bool) -> Placement;

/// A translucent surface with one floating card of `width` (a sheet of the bar): `fill`
/// adds the card's content; the card's height follows it, the surface gets the shadow
/// margins of `place`.
pub fn card_sheet(
    look: Look,
    screen: Size,
    width: f32,
    tr: &Translator,
    place: PlaceFn,
    fill: impl FnOnce(&mut View, NodeId) -> Result<(), UiError>,
) -> Result<(View, Placement), UiError> {
    let mut ui = new_ui(look, screen, tr)?;
    ui.set_transparent_background();
    ui.set_root_flex(Flex::column().cross(CrossAlign::Start));
    let flex = Flex::column()
        .padding(Padding::all(Space::S16))
        .gap(Space::S16);
    let card = Card::new().floating().flex(flex);
    let root = ui.root();
    let card = ui.add_sized(root, card, Sizing::width(width))?;
    let mut view = View::new(ui);
    fill(&mut view, card)?;
    let height = view.ui.content_size().h;
    let placement = place(
        screen,
        Size::new(width, height),
        look.lang.direction().is_rtl(),
    );
    view.ui
        .set_root_flex(Flex::column().cross(CrossAlign::Start).padding(Padding {
            start: Some(placement.start),
            end: None,
            top: Some(placement.top),
            bottom: None,
        }));
    view.ui.set_size(placement.surface)?;
    Ok((view, placement))
}
