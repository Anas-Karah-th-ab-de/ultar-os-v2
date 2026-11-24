//! Phone app of Ultar OS (area apps; DESKTOP-SPEC §12.2, boards PhoneDial, PhoneIncoming,
//! PhoneInCall). One app, two ways: the Ultar number (end-to-end encrypted) and the mobile
//! network (with a modem and a SIM).
//!
//! * [`model`] – what is shown (state of the call service, typed number, filter, dialogs);
//! * [`view`] – the toolkit tree of a model (`ultar-ui` widgets and tokens only);
//! * [`app::PhoneApp`] – input, clicks ([`click`], every action of `docs/ui/CLICKS.md`),
//!   requests to the call service through a [`port::CallPort`] (bus contract
//!   `ultar_bus::messages::call`) and platform requests through a [`port::Host`].
//!
//! Every user-visible text is a key of `locales/{ar,en,de}.ftl` (Arabic first).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
mod app_click;
pub mod click;
pub mod launch;
pub mod model;
pub mod port;
pub mod text;
pub mod view;
pub mod when;
pub mod window;

pub use app::{AppError, Look, PhoneApp};
pub use click::{Click, WindowOp};
pub use model::{Model, Screen};
pub use port::{BusPort, CallPort, Host, PortError, Request};
pub use window::{HostOp, PhoneWindow, QueuedHost, Step};
