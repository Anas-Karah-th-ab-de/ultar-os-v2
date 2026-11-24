//! `ultar-files` – the Files app of Ultar OS (area 6 "apps", plan §3.0; DESKTOP-SPEC §5.1,
//! §9, §13; boards "Main", "DesktopTop"): places, folders, the trash, mounted drives, and the
//! icons on the desktop.
//!
//! Security (APP-RULES §4/§5): the app runs in its launcher cell and sees only the named
//! areas the cell grants (`/areas/<name>`); [`fs`] walks them one name at a time from an
//! open directory and never follows a symbolic link, so nothing outside the areas can be
//! reached through them.
//!
//! Layers: [`fs`] (file operations, trash, undo, search, thumbnails – no UI), [`model`]
//! (state), [`update`] (clicks and keys → state + effects, no I/O), [`view`] (widget trees,
//! `ultar-ui` only), [`service`] (launcher and drive service as ports), [`jobs`]
//! (background threads), [`app`] (the glue).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod action;
pub mod app;
mod app_effects;
pub mod fs;
pub mod i18n;
pub mod jobs;
pub mod model;
pub mod service;
pub mod update;
pub mod uri;
pub mod view;

pub use app::{App, Mode, Ports};
