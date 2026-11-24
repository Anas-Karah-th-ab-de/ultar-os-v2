//! `ultar-desktop` – the desktop shell of Ultar OS (area 5, platform; DESKTOP-SPEC §5, §11,
//! boards `design/boards/project/*.dc.html`).
//!
//! Three programs, each its own compositor client, service and user:
//! - `ultar-desktop`: the one bar at the bottom (standard, §11), its sheets (quick settings,
//!   calendar and notifications, all apps), the overview (Super), the display-mode panel
//!   (Super+P), menus of the bar – role *panel* plus popups of the panel (right `ui.shell`).
//! - `ultar-lock`: the lock screen – role *lock* (right `ui.lock-screen`); it stays connected
//!   all session, shows itself on `SessionLocked`, checks the password with `ultar-authd`
//!   and only then sends `Unlock`.
//! - `ultar-sysdialog`: permission prompts, the administrator password prompt and "keep
//!   these display settings?" – role *system dialog* (right `ui.system-dialog`), inside the
//!   compositor's trusted frame.
//!
//! Layers inside the crate: [`model`] (state without drawing), [`view`] (widget trees built
//! from `ultar-ui` only), [`shell`] (what each click does), [`lock`] and [`dialog`] (their
//! state machines), [`ports`] (the services behind them, as traits with bus
//! implementations), [`host`] (compositor surfaces running a `Ui`).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
pub mod dialog;
pub mod host;
pub mod i18n;
pub mod lock;
pub mod model;
pub mod ports;
pub mod shell;
pub mod view;
