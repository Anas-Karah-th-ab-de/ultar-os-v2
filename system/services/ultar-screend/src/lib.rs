//! # ultar-screend - the screen service (remote view, mesh item C1)
//!
//! Shows this device's screen to a paired device and takes its pointer and keys. It never
//! starts by itself: the mesh service (`ultar-mesh`) lets a request through only from a
//! paired device with the right "control" and only after the person on this device said
//! yes in the system dialog; the mesh then tells this service to begin
//! (`mesh.view_serve`). While a session runs, a notification with a Stop button is shown
//! ([`notice`]); Stop, a lost link, taking the right back, unpairing and the lock of the
//! screen all end it, and every key or button the viewer held is released.
//!
//! The picture goes as rectangles that changed ([`diff`]), at most 30 times a second and
//! slower when the screen is quiet or the link is slow ([`pace`]); the mesh compresses them
//! (QOI, lossless) and does the flow control. The viewer's events are injected through
//! virtual input devices ([`input`]); without them the session is view only. The screen
//! is read from the compositor ([`screen`]); the whole logic of a session is in [`engine`]
//! over small traits, and [`service`] is its bus side.
//!
//! Rights of this service (own user): compositor `ui.capture` and kernel `input_inject`,
//! nothing else.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod diff;
pub mod engine;
pub mod input;
pub mod notice;
pub mod pace;
pub mod screen;
pub mod seat;
pub mod service;

pub use service::ScreendService;
