//! What a clickable element means (the action ids of `docs/ui/CLICKS.md`) and the registry
//! that maps widget nodes to them.

use ultar_bus::messages::account_service::AccountService;
use ultar_ui::ui::NodeId;

/// A clickable element of the app, by its CLICKS.md action (plus which one of several).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    /// `settings.search`.
    Search,
    /// `settings.area` (index into [`crate::areas::AREAS`]).
    Area(usize),
    /// `settings.back` (compact: area page → list of areas).
    Back,
    /// `display.select` (index of the display).
    DisplaySelect,
    /// `display.identify`.
    DisplayIdentify,
    /// `display.detect`.
    DisplayDetect,
    /// `display.mode` (radio group: extend, mirror, single).
    DisplayMode,
    /// Row "Mirrored displays": check box of display `n` (shown with 3+ displays in mirror).
    DisplayMirrorPick(usize),
    /// Row "Display that stays on" (mode "only one display").
    DisplaySingle,
    /// `display.nightlight`.
    DisplayNightlight,
    /// `display.nightlight.more`.
    DisplayNightlightMore,
    /// `display.remember`.
    DisplayRemember,
    /// `display.bar`.
    DisplayBar,
    /// `display.lid`.
    DisplayLid,
    /// Button "Make this the main display" (§13 no. 64).
    DisplayMakePrimary,
    /// `display.resolution`.
    DisplayResolution,
    /// `display.scale`.
    DisplayScale,
    /// `display.refresh`.
    DisplayRefresh,
    /// `display.orientation`.
    DisplayOrientation,
    /// `display.hdr`.
    DisplayHdr,
    /// `display.profile`.
    DisplayProfile,
    /// `display.brightness`.
    DisplayBrightness,
    /// `display.autobrightness` (only with a light sensor).
    DisplayAutoBrightness,
    /// Night light page: schedule choice.
    NightSchedule,
    /// Night light page: warmth slider.
    NightWarmth,
    /// Night light page: back to Displays.
    NightBack,
    /// `drivers.alt` of device row `n`.
    DriverAlt(usize),
    /// `drivers.rollback` of device row `n`.
    DriverRollback(usize),
    /// `drivers.plan` of device row `n`.
    DriverPlan(usize),
    /// `drivers.fw.check`.
    FirmwareCheck,
    /// `drivers.fw.usb`.
    FirmwareUsb,
    /// `drivers.selftest`.
    SelfTest,
    /// `account.back`.
    AccountBack,
    /// `account.username`.
    AccountUser,
    /// `account.password`.
    AccountPassword,
    /// `account.show`.
    AccountShow,
    /// `account.signin`.
    AccountSignIn,
    /// `account.create`.
    AccountCreate,
    /// `account.server`.
    AccountServer,
    /// `account.copy`.
    AccountCopy,
    /// `account.qr`.
    AccountQr,
    /// `account.service`.
    AccountServiceSwitch(AccountService),
    /// `account.changepw`.
    AccountChangePassword,
    /// `account.signout`.
    AccountSignOut,
    /// `account.delete`.
    AccountDelete,
    /// `account.device.signout` of device `n`.
    AccountDeviceSignOut(usize),
    /// `account.device.remove` of device `n`.
    AccountDeviceRemove(usize),
    /// Bluetooth page: the search switch.
    BtScan,
    /// Bluetooth page: pair with found device `n` (in the order shown).
    BtPair(usize),
    /// Mesh on or off.
    MeshToggle,
    /// Copy this device's id.
    MeshCopy,
    /// Show a pairing code.
    MeshPair,
    /// Open the page of the paired device with this index.
    MeshDevice(usize),
    /// Type the code of the nearby device with this index.
    MeshNearPair(usize),
    /// Back from a device's page.
    MeshBack,
    /// The right with this index ([`crate::state::mesh::GRANTS`]).
    MeshGrant(usize),
    /// Unpair the open device.
    MeshUnpair,
    /// A row of a generic settingsd panel (index into the shown items).
    PanelItem(usize),
    /// Users: "Add user".
    UserAdd,
    /// Users: remove user `n`.
    UserRemove(usize),
    /// Users: new password for user `n`.
    UserPassword(usize),
    /// Apps: the switch of permission `1` of app `0` (off takes it back).
    AppRevoke(usize, usize),
    /// A field of an open dialog (index).
    DialogField(usize),
    /// The confirming button of the open dialog.
    DialogOk,
    /// The cancelling button of the open dialog.
    DialogCancel,
}

impl Act {
    /// The CLICKS.md action id of the element (`None` for elements the boards do not show).
    pub fn id(self) -> Option<&'static str> {
        Some(match self {
            Act::Search => "settings.search",
            Act::Area(_) => "settings.area",
            Act::Back => "settings.back",
            Act::DisplaySelect => "display.select",
            Act::DisplayIdentify => "display.identify",
            Act::DisplayDetect => "display.detect",
            Act::DisplayMode => "display.mode",
            Act::DisplayNightlight => "display.nightlight",
            Act::DisplayNightlightMore => "display.nightlight.more",
            Act::DisplayRemember => "display.remember",
            Act::DisplayBar => "display.bar",
            Act::DisplayLid => "display.lid",
            Act::DisplayResolution => "display.resolution",
            Act::DisplayScale => "display.scale",
            Act::DisplayRefresh => "display.refresh",
            Act::DisplayOrientation => "display.orientation",
            Act::DisplayHdr => "display.hdr",
            Act::DisplayProfile => "display.profile",
            Act::DisplayBrightness => "display.brightness",
            Act::DisplayAutoBrightness => "display.autobrightness",
            Act::DriverAlt(_) => "drivers.alt",
            Act::DriverRollback(_) => "drivers.rollback",
            Act::DriverPlan(_) => "drivers.plan",
            Act::FirmwareCheck => "drivers.fw.check",
            Act::FirmwareUsb => "drivers.fw.usb",
            Act::SelfTest => "drivers.selftest",
            Act::AccountBack => "account.back",
            Act::AccountUser => "account.username",
            Act::AccountPassword => "account.password",
            Act::AccountShow => "account.show",
            Act::AccountSignIn => "account.signin",
            Act::AccountCreate => "account.create",
            Act::AccountServer => "account.server",
            Act::AccountCopy => "account.copy",
            Act::AccountQr => "account.qr",
            Act::AccountServiceSwitch(_) => "account.service",
            Act::AccountChangePassword => "account.changepw",
            Act::AccountSignOut => "account.signout",
            Act::AccountDelete => "account.delete",
            Act::AccountDeviceSignOut(_) => "account.device.signout",
            Act::AccountDeviceRemove(_) => "account.device.remove",
            Act::MeshToggle => "mesh.toggle",
            Act::MeshCopy => "mesh.copy",
            Act::MeshPair => "mesh.pair",
            Act::MeshDevice(_) => "mesh.device",
            Act::MeshNearPair(_) => "mesh.near.pair",
            Act::MeshBack => "mesh.back",
            Act::MeshGrant(_) => "mesh.right",
            Act::MeshUnpair => "mesh.unpair",
            _ => return None,
        })
    }
}

/// Most registered nodes (every clickable of a page plus the sidebar).
pub const MAX_REGISTERED: usize = 512;

/// Node → action map of the widget tree currently shown.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    entries: Vec<(NodeId, Act)>,
}

impl Registry {
    /// Remembers that `node` stands for `act` (silently ignores entries beyond the limit,
    /// which the view never reaches: pages hold far fewer clickables).
    pub fn add(&mut self, node: NodeId, act: Act) {
        if self.entries.len() < MAX_REGISTERED {
            self.entries.push((node, act));
        }
    }

    /// The action of a node.
    pub fn act(&self, node: NodeId) -> Option<Act> {
        self.entries
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, a)| *a)
    }

    /// The node of an action (first one).
    pub fn node(&self, act: Act) -> Option<NodeId> {
        self.entries
            .iter()
            .find(|(_, a)| *a == act)
            .map(|(n, _)| *n)
    }

    /// Every entry in registration order.
    pub fn entries(&self) -> &[(NodeId, Act)] {
        &self.entries
    }

    /// Forgets every entry whose node satisfies `gone` (the nodes were removed).
    pub fn retain(&mut self, mut keep: impl FnMut(NodeId, Act) -> bool) {
        self.entries.retain(|(n, a)| keep(*n, *a));
    }
}
