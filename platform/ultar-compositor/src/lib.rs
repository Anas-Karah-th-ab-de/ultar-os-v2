//! # ultar-compositor – the native compositor of Ultar OS
//!
//! **Area:** platform (area 5, plan §3.0; layer L4 in §3.0b). Shows the
//! windows of app *processes*: every app connects over a Unix socket
//! (`ultar-surface` protocol), its identity and rights come from the
//! kernel's peer credentials and the rights table – never from the client.
//! Pixels arrive in sealed shared memory (Linux) or inline (Ultar kernel),
//! are copied into compositor memory at commit, and are composed in
//! software, damage-based, onto the HAL displays (DRM/KMS, framebuffer) or
//! a headless backend. Title bars are drawn by the compositor with the
//! toolkit's tokens (buttons at the end, mirrored in Arabic).
//!
//! Design, security model and kernel gaps: `docs/COMPOSITOR.md`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod backend;
pub mod clock;
pub mod config;
pub mod conn;
pub mod cursor;
pub mod decor;
pub mod geom;
pub mod keymap;
pub mod msg;
pub mod output;
pub mod output_setup;
pub mod pixels;
pub mod render;
pub mod server;
pub mod state;
pub mod surface;
pub mod wm;

pub use config::{Config, Limits, RightsTable};
pub use server::{Clock, Compositor, MonotonicClock, Stats};
