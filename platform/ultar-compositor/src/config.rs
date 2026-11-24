//! Settings and limits of the compositor, and the rights table.

use crate::keymap::Layout as KeyLayout;
use std::collections::HashMap;
use ultar_surface::proto::Rights;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Lang;

/// Hard limits that keep one client from exhausting the compositor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Connected clients.
    pub clients: usize,
    /// Surfaces per client.
    pub surfaces: usize,
    /// Buffers per client.
    pub buffers: usize,
    /// Popups per toplevel window.
    pub popups: usize,
    /// Pixel memory per client (buffers + committed surface copies).
    pub client_bytes: u64,
    /// Unsent events per client before it is dropped as too slow.
    pub out_bytes: usize,
    /// Bytes read from one client per loop turn (fairness).
    pub read_slice: usize,
    /// Milliseconds without `Pong` before a window is "not responding".
    pub ping_ms: u64,
    /// Milliseconds a user-action serial stays valid.
    pub serial_ms: u64,
    /// What user ids share and what is kept for the system clients.
    pub share: ShareLimits,
    /// Clipboard and drag and drop.
    pub transfer: TransferLimits,
}

/// Limits per user id and the reserve of the system clients (shell, lock
/// screen, input method: uids holding `ui.shell`, `ui.lock-screen` or
/// `ui.input-method`). Every uid – system ones too – has the per-uid
/// limits; only system uids may use the reserves.
///
/// Until apps run under uids of their own (kernel gap K9) every app of the
/// session shares one uid, so the per-uid limits are the session's limits
/// for all apps together: they are set high (32 connections, 512 MiB) and
/// the per-connection limits do the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareLimits {
    /// Connections per uid.
    pub clients_per_uid: usize,
    /// Of [`Limits::clients`], the slots only system uids may take.
    pub reserved_clients: usize,
    /// Pixel memory per uid (system uids excepted).
    pub uid_bytes: u64,
    /// Pixel memory of all clients together.
    pub total_bytes: u64,
    /// Of `total_bytes`, the part only system uids may use (a lock screen
    /// always gets its buffers).
    pub reserved_bytes: u64,
    /// Visible toplevel windows per uid without rights (more are left out
    /// of the scene: frame cost).
    pub windows_per_uid: usize,
}

/// Limits of clipboard and drag-and-drop transfers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferLimits {
    /// Size of one transfer.
    pub bytes: usize,
    /// Bytes queued for a transfer's reader before further data of that
    /// transfer is held back (flow control per transfer).
    pub queue: usize,
    /// Bytes of one transfer the compositor holds back at most; a source
    /// that sends more without being asked (`SendRequest`) loses the
    /// transfer.
    pub held: usize,
    /// Milliseconds a transfer may take.
    pub ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            clients: 64,
            surfaces: 64,
            buffers: 64,
            popups: 8,
            client_bytes: 256 << 20,
            out_bytes: 8 << 20,
            read_slice: 2 << 20,
            ping_ms: 5_000,
            serial_ms: 10_000,
            share: ShareLimits {
                clients_per_uid: 32,
                reserved_clients: 8,
                uid_bytes: 512 << 20,
                total_bytes: 1 << 30,
                reserved_bytes: 128 << 20,
                windows_per_uid: 32,
            },
            transfer: TransferLimits {
                bytes: 16 << 20,
                queue: 2 << 20,
                held: 2 << 20,
                ms: 10_000,
            },
        }
    }
}

/// Settings of one compositor instance.
#[derive(Clone, Debug)]
pub struct Config {
    /// System language (title bars, direction of the decorations).
    pub lang: Lang,
    /// Colour theme.
    pub theme: ThemeKind,
    /// Keyboard layouts, the first is active at start.
    pub layouts: Vec<KeyLayout>,
    /// Limits.
    pub limits: Limits,
    /// Send pixels inline even where shared memory works (tests, kernels
    /// without memfd).
    pub inline_only: bool,
    /// Rights by user id.
    pub rights: RightsTable,
    /// Draw the pointer (off for touch-only devices and pixel tests).
    pub cursor: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            lang: Lang::Ar,
            theme: ThemeKind::Dark,
            layouts: vec![KeyLayout::Arabic, KeyLayout::English],
            limits: Limits::default(),
            inline_only: false,
            rights: RightsTable::default(),
            cursor: true,
        }
    }
}

/// Why a rights file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RightsError {
    /// A line is not `uid = "right,right"`.
    Syntax(usize),
    /// A right name is unknown.
    UnknownName(usize),
    /// A user id appears twice.
    Duplicate(usize),
    /// Rights for root (0) or the unknown id (`u32::MAX`) without
    /// `--allow-root`: services and apps have ids of their own.
    Privileged(usize),
}

impl std::fmt::Display for RightsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RightsError::Syntax(line) => write!(f, "compositor.rights.syntax ({line})"),
            RightsError::UnknownName(line) => write!(f, "compositor.rights.unknown ({line})"),
            RightsError::Duplicate(line) => write!(f, "compositor.rights.duplicate ({line})"),
            RightsError::Privileged(line) => write!(f, "compositor.rights.privileged ({line})"),
        }
    }
}

impl std::error::Error for RightsError {}

/// Rights per user id (every app runs under its own id in its cell; the
/// app installer writes the ids of apps whose manifest grants `ui.*`
/// rights). Unknown ids get [`Rights::NONE`]: default deny.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RightsTable {
    by_uid: HashMap<u32, Rights>,
    /// The app launcher's file for app user ids (`--app-rights`).
    apps: Option<Box<crate::app_rights::AppRights>>,
}

/// Manifest name of each right.
pub const PERMISSION_NAMES: [(&str, Rights); 9] = [
    ("ui.shell", Rights::SHELL),
    ("ui.input-method", Rights::INPUT_METHOD),
    ("ui.lock-screen", Rights::LOCK_SCREEN),
    ("ui.capture", Rights::CAPTURE),
    ("ui.overlay", Rights::OVERLAY),
    ("ui.system-dialog", Rights::SYSTEM_DIALOG),
    ("ui.output-config", Rights::OUTPUT_CONFIG),
    ("ui.call-screen", Rights::CALL_SCREEN),
    ("ui.client-header", Rights::CLIENT_HEADER),
];

impl RightsTable {
    /// Parses lines `1001 = "ui.shell,ui.capture"`; `#` starts a comment.
    /// Root and the unknown id are refused (see [`RightsTable::parse_with`]).
    pub fn parse(text: &str) -> Result<RightsTable, RightsError> {
        RightsTable::parse_with(text, false)
    }

    /// Parses a rights file; `allow_root` accepts rights for uid 0 (for
    /// development systems where everything runs as root). A user id may
    /// appear once; `u32::MAX` (credentials unknown) never gets rights.
    pub fn parse_with(text: &str, allow_root: bool) -> Result<RightsTable, RightsError> {
        let mut table = RightsTable::default();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (uid, value) = line.split_once('=').ok_or(RightsError::Syntax(n + 1))?;
            let uid: u32 = uid.trim().parse().map_err(|_| RightsError::Syntax(n + 1))?;
            if uid == u32::MAX || (uid == 0 && !allow_root) {
                return Err(RightsError::Privileged(n + 1));
            }
            if table.by_uid.contains_key(&uid) {
                return Err(RightsError::Duplicate(n + 1));
            }
            let value = value.trim();
            let names = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .ok_or(RightsError::Syntax(n + 1))?;
            let mut rights = Rights::NONE;
            for name in names.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (_, granted) = PERMISSION_NAMES
                    .iter()
                    .find(|(k, _)| *k == name)
                    .ok_or(RightsError::UnknownName(n + 1))?;
                rights = rights.union(*granted);
            }
            table.by_uid.insert(uid, rights);
        }
        Ok(table)
    }

    /// Grants rights to a user id.
    pub fn grant(&mut self, uid: u32, rights: Rights) {
        let entry = self.by_uid.entry(uid).or_default();
        *entry = entry.union(rights);
    }

    /// The rights of a user id: from this table, else from the app
    /// launcher's file for app ids.
    pub fn rights_of(&self, uid: u32) -> Rights {
        match (self.by_uid.get(&uid), &self.apps) {
            (Some(rights), _) => *rights,
            (None, Some(apps)) => apps.rights_of(uid),
            (None, None) => Rights::NONE,
        }
    }

    /// The user ids this table names.
    pub fn uids(&self) -> impl Iterator<Item = u32> + '_ {
        self.by_uid.keys().copied()
    }

    /// Adds the app launcher's rights file for app user ids.
    pub fn set_apps(&mut self, apps: crate::app_rights::AppRights) {
        self.apps = Some(Box::new(apps));
    }

    /// Reads the app launcher's file again if it changed (at every hello).
    pub fn refresh_apps(&mut self) {
        if let Some(apps) = &mut self.apps {
            apps.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights_file() {
        let table = RightsTable::parse(
            "# shell and screenshot tool\n1001 = \"ui.shell, ui.capture\"\n\n1002 = \"\"\n1003=\"ui.input-method\" # keyboard\n",
        )
        .unwrap();
        assert_eq!(table.rights_of(1001), Rights::SHELL.union(Rights::CAPTURE));
        assert_eq!(table.rights_of(1002), Rights::NONE);
        assert_eq!(table.rights_of(1003), Rights::INPUT_METHOD);
        assert_eq!(table.rights_of(4242), Rights::NONE);
        assert_eq!(RightsTable::parse("x = \"\""), Err(RightsError::Syntax(1)));
        assert_eq!(
            RightsTable::parse("1 = ui.shell"),
            Err(RightsError::Syntax(1))
        );
        assert_eq!(
            RightsTable::parse("1 \"ui.shell\""),
            Err(RightsError::Syntax(1))
        );
        assert_eq!(
            RightsTable::parse("\n1 = \"ui.root\""),
            Err(RightsError::UnknownName(2))
        );
        assert!(RightsError::Syntax(3).to_string().contains('3'));
        assert_eq!(
            RightsTable::parse("7 = \"\"\n7 = \"ui.shell\""),
            Err(RightsError::Duplicate(2))
        );
        assert_eq!(
            RightsTable::parse("0 = \"ui.capture\""),
            Err(RightsError::Privileged(1))
        );
        assert_eq!(
            RightsTable::parse_with("4294967295 = \"\"", true),
            Err(RightsError::Privileged(1))
        );
        let root = RightsTable::parse_with("0 = \"ui.system-dialog\"", true).unwrap();
        assert_eq!(root.rights_of(0), Rights::SYSTEM_DIALOG);
        let mut t = RightsTable::default();
        t.grant(5, Rights::LOCK_SCREEN);
        t.grant(5, Rights::OVERLAY);
        assert_eq!(t.rights_of(5), Rights::LOCK_SCREEN.union(Rights::OVERLAY));
        let c = Config::default();
        assert_eq!(c.lang, Lang::Ar);
        assert_eq!(c.layouts[0], KeyLayout::Arabic);
        assert!(c.limits.client_bytes > 0);
    }
}
