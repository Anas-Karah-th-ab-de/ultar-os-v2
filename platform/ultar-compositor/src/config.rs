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
    /// Pixel memory per client (buffers + committed surface copies).
    pub client_bytes: u64,
    /// Unsent events per client before it is dropped as too slow.
    pub out_bytes: usize,
    /// Bytes read from one client per loop turn (fairness).
    pub read_slice: usize,
    /// Size of one clipboard or drop transfer.
    pub transfer_bytes: usize,
    /// Milliseconds a transfer may take.
    pub transfer_ms: u64,
    /// Milliseconds without `Pong` before a window is "not responding".
    pub ping_ms: u64,
    /// Milliseconds a user-action serial stays valid.
    pub serial_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            clients: 64,
            surfaces: 64,
            buffers: 64,
            client_bytes: 256 << 20,
            out_bytes: 8 << 20,
            read_slice: 2 << 20,
            transfer_bytes: 16 << 20,
            transfer_ms: 10_000,
            ping_ms: 5_000,
            serial_ms: 10_000,
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
}

impl std::fmt::Display for RightsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RightsError::Syntax(line) => write!(f, "compositor.rights.syntax ({line})"),
            RightsError::UnknownName(line) => write!(f, "compositor.rights.unknown ({line})"),
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
}

/// Manifest name of each right.
pub const PERMISSION_NAMES: [(&str, Rights); 5] = [
    ("ui.shell", Rights::SHELL),
    ("ui.input-method", Rights::INPUT_METHOD),
    ("ui.lock-screen", Rights::LOCK_SCREEN),
    ("ui.capture", Rights::CAPTURE),
    ("ui.overlay", Rights::OVERLAY),
];

impl RightsTable {
    /// Parses lines `1001 = "ui.shell,ui.capture"`; `#` starts a comment.
    pub fn parse(text: &str) -> Result<RightsTable, RightsError> {
        let mut table = RightsTable::default();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (uid, value) = line.split_once('=').ok_or(RightsError::Syntax(n + 1))?;
            let uid: u32 = uid.trim().parse().map_err(|_| RightsError::Syntax(n + 1))?;
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

    /// The rights of a user id.
    pub fn rights_of(&self, uid: u32) -> Rights {
        self.by_uid.get(&uid).copied().unwrap_or(Rights::NONE)
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
