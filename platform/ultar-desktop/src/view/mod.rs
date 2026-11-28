//! Widget trees of the desktop, built from `ultar-ui` bricks and widgets only, with the
//! geometry of the boards (`design/boards/gen.py`: `bottombar`, `sheet`, `qt`, `notif`,
//! the overview and the projection panel). Every operable node is registered with a
//! [`Target`]: what it means for the shell.

pub mod apps;
pub mod banners;
pub mod bar;
pub mod bluetooth;
pub mod calendar;
pub mod desk;
pub mod dock;
pub mod geometry;
pub mod helper;
pub mod menu;
pub mod note;
pub mod overview;
pub mod projection;
pub mod quick;
pub mod wallpaper;
pub mod wifi;

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
    /// Helper sheet: send the request.
    HelperSend,
    /// Helper sheet: stop waiting for the answer.
    HelperStop,
    /// Helper sheet: open Settings › Assistant.
    HelperSettings,
    /// Quick settings: open the helper (`qs.assistant`).
    Assistant,
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
    /// Calendar: an appointment of the shown day (cal.event).
    Event(u64),
    /// Bar: a mounted drive (shell.drive), by its index in `Services::drives`.
    Drive(usize),
    /// Bluetooth page: the search switch.
    BtScan,
    /// Bluetooth page: a device (pair, connect or disconnect).
    BtDevice(String),
    /// Bluetooth page: forget a paired device.
    BtForget(String),
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
    /// Overview: a workspace (from 0; the last one adds a workspace).
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
    /// Lock: emergency panel – call the emergency number (`call.emergency`).
    EmergencyCall,
    /// Lock: emergency panel – back to the cover.
    EmergencyBack,
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
    /// Quick settings: the Wi-Fi radio's switch on the networks page.
    WifiRadio,
    /// Quick settings: a Wi-Fi network (its name).
    WifiNetwork(String),
    /// Quick settings: forget a saved network.
    WifiForget(String),
    /// Quick settings: "Wi-Fi settings" (Settings > Wi-Fi).
    WifiSettings,
    /// Join dialog: the password field.
    JoinPassword,
    /// Join dialog: "save this network".
    JoinRemember,
    /// Join dialog: connect.
    JoinConnect,
    /// Join dialog: cancel.
    JoinCancel,
    /// Quick settings: the VPN's switch on the connections page.
    VpnSwitch,
    /// Quick settings: a VPN connection (its name).
    VpnProfile(String),
    /// Quick settings: "VPN settings" (Settings > Network > VPN).
    VpnSettings,
    /// Quick settings: the accessibility button (Settings > Accessibility), shown
    /// while `accessibility.show_in_bar` is on.
    AccessSettings,
    /// Lock: accessibility menu.
    Access,
    /// Lock: a switch of the accessibility menu (index into `lock::access::SWITCHES`).
    AccessSwitch(u8),
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
    /// Top bar: the menu of the active app.
    AppMenu,
    /// On-screen keyboard: a key (row, index in the row of `ultar_ime::rows`).
    OskKey(usize, usize),
    /// On-screen keyboard: a word suggestion (index).
    OskSuggestion(usize),
    /// A notification's action button (notification id, action index).
    NoteAction(u64, u8),
    /// Lock screen: a control of the playing medium (0 previous, 1 play/pause, 2 next).
    Media(u8),
    /// Desktop background: an icon (home folder, drive, app shortcut, desktop folder item).
    Desk(crate::model::places::DeskItem),
    /// Rename dialog of a shortcut: the name field.
    RenameField,
    /// Rename dialog: rename.
    RenameSave,
    /// Rename dialog: cancel.
    RenameCancel,
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
    /// accessibility.high_contrast.
    pub high_contrast: bool,
    /// accessibility.large_text: text this many times larger (1 = off).
    pub text_scale: f32,
    /// accessibility.reduce_motion: knobs jump, progress rings stand still.
    pub reduce_motion: bool,
    /// accessibility.double_click_ms: two clicks of a double click this far apart (ms).
    pub double_click_ms: u16,
    /// appearance.accent_color.
    pub accent: ultar_ui::tokens::Accent,
}

/// Text factor of accessibility.large_text.
pub const LARGE_TEXT: f32 = 1.25;

impl Look {
    /// With the accessibility settings `high_contrast` and `large_text` applied.
    pub fn with_access(mut self, high_contrast: bool, large_text: bool) -> Look {
        self.high_contrast = high_contrast;
        self.text_scale = if large_text { LARGE_TEXT } else { 1.0 };
        self
    }

    /// With the person's preferences applied: the accent colour and the text size
    /// (`appearance.font_size`, on top of `large_text`).
    pub fn with_prefs(mut self, prefs: &crate::model::prefs::Prefs) -> Look {
        self.accent = prefs.accent;
        self.text_scale *= prefs.font_factor();
        self
    }

    /// Pointer input at scale 1.
    pub fn new(lang: Lang, theme: ThemeKind) -> Look {
        Look {
            lang,
            theme,
            scale: 1.0,
            mode: InputMode::Pointer,
            high_contrast: false,
            text_scale: 1.0,
            reduce_motion: false,
            double_click_ms: ultar_ui::tokens::motion::DOUBLE_CLICK as u16,
            accent: ultar_ui::tokens::Accent::Gold,
        }
    }
}

/// An empty tree of `size` in a look.
pub fn new_ui(look: Look, size: Size, tr: &Translator) -> Result<Ui, UiError> {
    let mut env = PaintEnv::new(look.lang, look.theme);
    env.scale = look.scale;
    env.high_contrast = look.high_contrast;
    env.text_scale = look.text_scale;
    env.accent = look.accent;
    let mut ui = Ui::new(env, look.mode, size, tr.clone())?;
    ui.set_reduce_motion(look.reduce_motion);
    ui.set_double_click_ms(look.double_click_ms);
    Ok(ui)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_carries_the_double_click_time_into_the_window() {
        let tr = crate::i18n::translator(Lang::En).expect("catalogs");
        let mut look = Look::new(Lang::En, ThemeKind::Dark);
        assert_eq!(look.double_click_ms, 400);
        look.double_click_ms = 700;
        let ui = new_ui(look, Size::new(100.0, 100.0), &tr).expect("ui");
        assert_eq!(ui.double_click_ms(), 700);
    }
}
