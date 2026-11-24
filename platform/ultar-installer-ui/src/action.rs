//! The clickable elements (CLICKS.md `installer.*`) and the node → action registry.

use crate::components::Component;
use crate::wizard::Step;
use ultar_ui::ui::NodeId;

/// A clickable element of the installer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    /// `installer.step` (the navigation list; the item's step).
    Step(Step),
    /// `installer.scope`.
    Scope,
    /// `installer.component`.
    Component(Component),
    /// `installer.cancel`.
    Cancel,
    /// `installer.prev`.
    Prev,
    /// `installer.next` (on the summary: Install).
    Next,
    /// `installer.disk.select`.
    DiskSelect,
    /// `installer.disk.mode`.
    DiskMode,
    /// `installer.disk.share`.
    DiskShare,
    /// `installer.enc.mode`.
    EncMode,
    /// `installer.enc.pass` (0: passphrase, 1: repetition).
    EncPass(usize),
    /// `installer.drivers.fetch`.
    DriversFetch,
    /// `installer.drivers.plan` (`None`: all devices).
    DriversPlan(Option<usize>),
    /// `installer.drivers.alt` of row `n`.
    DriversAlt(usize),
    /// `installer.drivers.test` of row `n`.
    DriversTest(usize),
    /// `installer.edit`.
    Edit(Step),
    /// `installer.confirm`.
    Confirm,
    /// Step 1: language.
    Lang,
    /// Step 2: accessibility switch `n`.
    Access(usize),
    /// Step 3: keyboard.
    Keyboard,
    /// Step 5: try or install.
    TryOrInstall,
    /// Step 6: desktop or server.
    Kind,
    /// Step 9: extra `n`.
    Extra(usize),
    /// Step 12: field `n` (full name, user, device).
    AccountField(usize),
    /// Step 13: time zone.
    TimeZone,
    /// Step 16: restart.
    Restart,
    /// Cancel dialog: yes, cancel the installation.
    CancelYes,
    /// Cancel dialog: no, go on.
    CancelNo,
    /// `installer.enc.show`: shows or hides both passphrase fields.
    EncShow,
    /// Step 12: the user's password (0) and its repetition (1).
    Password(usize),
    /// Manual partitioning: the role of partition `i`.
    ManualRole(usize),
    /// Manual partitioning: delete partition `i`.
    ManualDelete(usize),
    /// Manual partitioning: size of a new partition (MiB).
    ManualSize,
    /// Manual partitioning: create the new partition.
    ManualCreate,
}

impl Act {
    /// CLICKS.md action id (`None` for elements without a board).
    pub fn id(self) -> Option<&'static str> {
        Some(match self {
            Act::Step(_) => "installer.step",
            Act::Scope => "installer.scope",
            Act::Component(_) => "installer.component",
            Act::Cancel => "installer.cancel",
            Act::Prev => "installer.prev",
            Act::Next => "installer.next",
            Act::DiskSelect => "installer.disk.select",
            Act::DiskMode => "installer.disk.mode",
            Act::DiskShare => "installer.disk.share",
            Act::EncMode => "installer.enc.mode",
            Act::EncPass(_) => "installer.enc.pass",
            Act::EncShow => "installer.enc.show",
            Act::DriversFetch => "installer.drivers.fetch",
            Act::DriversPlan(_) => "installer.drivers.plan",
            Act::DriversAlt(_) => "installer.drivers.alt",
            Act::DriversTest(_) => "installer.drivers.test",
            Act::Edit(_) => "installer.edit",
            Act::Confirm => "installer.confirm",
            _ => return None,
        })
    }
}

/// Node → action of the shown tree.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    entries: Vec<(NodeId, Act)>,
}

impl Registry {
    /// Most entries.
    pub const MAX: usize = 512;

    /// Registers a node.
    pub fn add(&mut self, node: NodeId, act: Act) {
        if self.entries.len() < Registry::MAX {
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

    /// The node of an action.
    pub fn node(&self, act: Act) -> Option<NodeId> {
        self.entries
            .iter()
            .find(|(_, a)| *a == act)
            .map(|(n, _)| *n)
    }

    /// All entries.
    pub fn entries(&self) -> &[(NodeId, Act)] {
        &self.entries
    }
}
