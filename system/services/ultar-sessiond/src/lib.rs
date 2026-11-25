//! `ultar-sessiond` – the session service of Ultar OS (area 4, system services).
//!
//! Keeps who is signed in on the seat and who is in front (`session.state`, retained,
//! also kept in a state file below `/run` so a restarted service does not forget open
//! sessions – after a power cycle nobody is signed in). Only the sign-in screen may
//! report a sign-in (pseudo method `session.signin`), after it checked the password
//! with the authentication service; the service checks that the account exists.
//! Switching users and signing out lock the screen (`lock.lock`) so the sign-in screen
//! is shown; the account service learns who is in front (`account.session`).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod parental;
mod parental_service;
pub mod service;
pub mod sessions;

pub use service::SessionService;
pub use sessions::Sessions;
