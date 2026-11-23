//! # ultar-surface – the client side of the Ultar compositor
//!
//! **Area:** platform (area 5, plan §3.0; layer L4/L5 boundary). Every app
//! process shows its windows through this crate: it speaks the compositor
//! protocol ([`proto`], framed by [`wire`]) over a Unix socket, allocates the
//! pixel buffers (sealed shared memory where the kernel has it, inline
//! pixels otherwise) and turns compositor events into input for the app.
//! With the feature `ui` (default) [`ui::UiWindow`] runs an `ultar-ui`
//! widget tree in its own window.
//!
//! Design and security model: `docs/COMPOSITOR.md`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod client;
pub mod proto;
#[cfg(feature = "ui")]
pub mod ui;
pub mod wire;
