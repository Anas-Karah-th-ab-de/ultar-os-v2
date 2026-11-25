//! The lock screen of the desktop (DESKTOP-SPEC §5.3, boards Lock and Login): its views.
//! The state machine is shared with the phone (`ultar-lock-core`).

pub mod access;
pub mod view;

pub use ultar_lock_core::{LockEffect, LockServices, LockState, SignInError, Stage};
