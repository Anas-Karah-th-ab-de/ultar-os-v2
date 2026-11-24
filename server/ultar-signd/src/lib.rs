//! `ultar-signd` – the release signing service of Ultar OS (area: server tooling, plan §3.0).
//!
//! The signing keys live on the release server (owner's decision). To keep them as safe as a
//! server allows, this crate is the **only** program that ever reads them:
//!
//! * it runs as its own user (`ultar-sign`) and refuses to start as root;
//! * key files must be readable by that user only (`0400`/`0600`), anything wider is refused;
//! * three separate keys ([`keys::Role`]): OS releases, system apps, store apps and catalogue;
//! * it listens on a local Unix socket only (no network), and signs **digests in a fixed,
//!   domain-separated message layout** that it builds itself ([`request`]) – a caller can never
//!   make it sign arbitrary bytes, and a signature of one kind can never pass as another kind;
//! * every request is rate-limited per key ([`ratelimit`]) and written to an audit log
//!   ([`audit`]) with the caller's uid, the digest and the result.
//!
//! The same binary is also the client used by the publish scripts ([`client`]): it computes
//! the digests, asks the socket and writes the signed `.uap`, the catalogue `.sig` and the
//! release `.sig` (OpenSSH `SSHSIG` format, so anybody can verify a download with
//! `ssh-keygen -Y verify`, see [`sshsig`]).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod audit;
pub mod client;
pub mod error;
pub mod keys;
pub mod msg;
pub mod ratelimit;
pub mod request;
pub mod server;
pub mod sshsig;

pub use error::SignError;
