//! The system dialogs (DESKTOP-SPEC §5.3 "Systemdialoge"): the UI-free state lives in
//! `ultar-dialog-core` (shared with the phone), the desktop views here.

pub mod handoff;
pub mod view;

pub use ultar_dialog_core::*;
