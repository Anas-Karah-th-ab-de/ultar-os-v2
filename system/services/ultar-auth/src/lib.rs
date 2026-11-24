//! `ultar-auth` – the authentication service of Ultar OS (area 4, system services).
//!
//! It keeps the accounts that can sign in (name, display name, administrator, an
//! **Argon2id** hash of the password and optionally of a PIN) and answers the bus service
//! `auth` (`ultar_bus::messages::auth`): `users`, `verify`, `set_secret`. The lock screen,
//! the login screen and the system dialogs ask it; nobody else sees a hash.
//!
//! Rules:
//! - Hashing and verification are the `argon2` crate's (no own cryptography); the
//!   comparison inside it is constant-time. Unknown users and missing PINs are checked
//!   against a dummy hash, so the answer and its time do not tell them apart.
//! - Every wrong try doubles the wait before the next one (1 s, 2 s, 4 s … 5 min per user);
//!   the counters are stored, so a restart does not reset them.
//! - Files are written atomically (temp file, fsync, rename, directory fsync) and carry a
//!   CRC-32; a damaged account file is never guessed at: the service answers
//!   `Unavailable` (fail closed) until an administrator repairs it.
//! - Secrets are never logged, printed or put in an error; they are wiped after use.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod clock;
pub mod file;
pub mod hash;
pub mod msg;
pub mod name;
pub mod server;
pub mod service;
pub mod store;
pub mod throttle;

pub use clock::{Clock, ManualClock, SystemClock};
pub use service::{Auth, AuthConfig};
