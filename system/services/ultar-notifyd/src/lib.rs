//! `ultar-notifyd` – the notification service of Ultar OS (area 4, system services).
//!
//! Apps and services post notifications (`notify.post`, up to three buttons); the
//! service checks and numbers them, names the sender by its bus identity (it cannot be
//! forged) and publishes `notify.posted`. The shell lists the open ones
//! (`notify.list`), closes them (`notify.dismiss`) and presses buttons
//! (`notify.invoke`); the sender then gets the hint `notify.invoked` and fetches only its
//! own pressed buttons (`notify.take_invoked`).
//!
//! Limits: 64 open notifications (the oldest of the lowest urgency goes first), 30
//! posts per minute and sender, 16 waiting pressed buttons per sender, 64 senders with
//! waiting buttons, text sizes below. Open notifications live in memory: a restarted
//! service starts empty (the shell's banners stay until closed).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod notes;
pub mod service;

pub use notes::Notes;
pub use service::NotifyService;
