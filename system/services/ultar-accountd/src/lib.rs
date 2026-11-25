//! # ultar-accountd – the account service of Ultar OS
//!
//! **Area:** system services (area 4, plan §3.0). Owns `account.*`
//! (`ultar_bus::messages::{account_local, account_service}`):
//!
//! * **local users** (DESKTOP-SPEC §8 "Benutzer", §6 step 12): create,
//!   display name, picture, password, administrator or standard, automatic
//!   sign-in, remove with the choice to keep or delete the files; topic
//!   `account.users` (retained). Rules and checks in [`core`].
//! * **the Ultar account link** (§12.1) to the account server `ultraos-server`
//!   ([`link`], server port [`link_cloud`], files [`link_store`]): sign in or create
//!   an account, this device's number, services, devices, password; the device
//!   stays fully usable without it.
//!
//! **One credential store.** Password hashes (Argon2id) live only in the
//! authentication service `ultar-authd` (`auth.*`), which the lock screen
//! and the system dialog use too. This service keeps no secret: it hands
//! passwords to `auth.set_secret` / `auth.verify` and keeps names, ids,
//! kinds and pictures in its own file ([`store`]).
//!
//! **Administrator actions** need an administrator's password in the
//! system's own dialog (`sysdialog.authenticate`), asked by this service
//! itself – the app that asks for the change cannot skip or fake it.
//! Limits: changes per caller ([`throttle`]), waits after wrong current
//! passwords (here and in `ultar-authd`), at most one open question.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod core;
pub mod daemon;
pub mod link;
pub mod link_cloud;
pub mod link_store;
pub mod msg;
pub mod names;
pub mod picture;
pub mod ports;
pub mod refusal;
pub mod store;
pub mod throttle;
