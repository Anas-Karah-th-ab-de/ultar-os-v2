//! # ultar-remote - see and operate the screen of another device
//!
//! The viewer of the remote view (mesh item C1). The device that is looked at runs
//! `ultar-screend`; the mesh carries the picture and the input. This app is the window:
//! [`app`] (choice, waiting, the picture with its toolbar, the end states), [`picture`] (the
//! remote frame buffer, scaling, pointer mapping), [`keys`] (keys as key codes of the other
//! device) and [`port`] (the way to the mesh over the bus).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod i18n;
pub mod keys;
pub mod picture;
pub mod port;
