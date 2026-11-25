//! `ultar-mobile` – the phone shell of Ultar OS (area 5, platform).
//!
//! It is built on its own and shares no code with the desktop shell (`ultar-desktop`); both
//! only use the common toolkit (`ultar-ui`) and the language base (`ultar-i18n`). The layout
//! follows the phone of the first Ultar OS ("Ultar Noir", `docs/MOBILE.md`):
//!
//! * lock screen – dot-matrix clock, date and Hijri date, notifications, torch · unlock · camera;
//! * home screen – brand mark and dot-matrix clock, date line, the cards "next" and "device",
//!   search, app grid in pages with dots, the dock;
//! * quick settings – the two large tiles (Wi-Fi, mobile data), round switches, brightness,
//!   notifications;
//! * recent apps, search, and the navigation bar (recent apps · home · back).
//!
//! Layers: [`model`] (state without drawing), [`view`] (widget trees from `ultar-ui` only),
//! [`click`] (what each operable node means), [`app`] (input → model → effects for the
//! platform). Every user-visible text is a key of `locales/{ar,en,de}.ftl`, Arabic first.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
pub mod click;
pub mod i18n;
pub mod model;
pub mod program;
pub mod view;

pub use app::{Effect, Look, MobileApp};
pub use click::Click;
pub use model::{Mobile, Screen};
