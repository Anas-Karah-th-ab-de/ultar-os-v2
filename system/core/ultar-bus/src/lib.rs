//! # ultar-bus – the one typed IPC of Ultar OS
//!
//! **Area:** system core (area 3 of the seven areas, plan §3.0). The bus is
//! the only way system core, system services, platform and apps talk to each
//! other. It replaces state files in `/run` that were polled in timer loops
//! (plan §3.6): everything here is event driven, and an idle bus causes no
//! wakeups at all.
//!
//! ## Building blocks
//!
//! * [`Broker`] – accepts connections on Unix domain sockets, authenticates
//!   peers ([`PeerIdentity`]), enforces the [`Policy`] (default deny) and
//!   routes. The `ultar-busd` binary runs one.
//! * [`Client`] – request/response with timeouts, publish/subscribe with
//!   hierarchical topics and prefix subscriptions, retained last values,
//!   automatic reconnection.
//! * [`Message`], [`Topic`], [`Call`] – contracts as versioned Rust types;
//!   the first real ones live in [`messages`].
//!
//! ## Guarantees
//!
//! * Events of one topic reach every subscriber in the same order.
//! * Queues are bounded. A subscriber that does not keep up is dropped with
//!   the typed error `SlowConsumer`; it never blocks the broker or others.
//! * Malformed, oversized or hostile input ends that one connection only.
//! * System apps and third-party apps use the same API; only the policy
//!   rules that match their identity differ.
//!
//! ## Portability
//!
//! Only `std` is used (`std::os::unix::net`, threads, blocking I/O), no
//! async runtime and no C code, so the crate runs unchanged on Linux and on
//! the Ultar kernel with its Linux-compatible system calls.
//!
//! ## Example
//!
//! ```no_run
//! use std::{path::Path, sync::Arc, time::Duration};
//! use ultar_bus::messages::power::PowerState;
//! use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
//!
//! # fn main() -> Result<(), ultar_bus::BusError> {
//! let socket = Path::new("/run/ultar/bus/system.sock");
//! let system = Arc::new(FixedIdentity::new(Identity::new("system")?));
//! let _broker = Broker::builder(Policy::allow_all()).listen(socket, system).start()?;
//!
//! let client = Client::connect(socket, ClientOptions::named("example"))?;
//! let power = client.subscribe::<PowerState>()?;
//! let state = power.recv_timeout(Duration::from_secs(1))?;
//! # let _ = state;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

mod broker;
mod client;
mod error;
mod identity;
mod message;
pub mod messages;
pub mod msg;
mod policy;
mod policy_text;
mod sync;
mod topic;
pub mod wire;

pub use broker::{Broker, BrokerBuilder, BrokerConfig, BrokerStats};
pub use client::{
    Client, ClientOptions, IncomingRequest, RawEvent, ServiceEndpoint, Subscription,
    TypedSubscription,
};
pub use error::{BusError, ErrorCode};
pub use identity::{
    CredentialIdentity, CredentialSource, FixedIdentity, Identity, PeerCredentials, PeerIdentity,
    PeerInfo, StaticCredentials, TokenIdentity,
};
pub use message::{decode, decode_versioned, encode, Call, Message, Text, TextArg, Topic};
pub use policy::{Policy, Rule};
pub use policy_text::{PolicyError, PolicyErrorKind};
pub use topic::{is_valid_name, is_valid_segment_name, Pattern, MAX_NAME_LEN};
