//! `ultar-mediad` – the media service of Ultar OS (area 4, system services).
//!
//! Holds the media session: the latest player that reported (`media.update`) holds it
//! and must report again at least every 30 s (`HEARTBEAT_S`), else it counts as gone
//! and the card disappears. `media.state` is retained; the buttons of the shell and the
//! lock screen (`media.play_pause` / `next` / `previous`, pseudo method `media.system`)
//! go to the player as `media.command` naming it.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod service;
pub mod session;

pub use service::MediaService;
pub use session::Session;
