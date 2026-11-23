//! # ultar-init – the service manager of Ultar OS
//!
//! PID 1 (`main.rs`: mounts, devices, the emergency exit) and, in this
//! library, the part that runs the services:
//!
//! * service files ([`spec`]) with identity, rights, memory group and
//!   limits per service; the start order ([`order`]);
//! * every service started confined by init's own launcher ([`launch`]):
//!   its memory group ([`groups`]), its user and groups, only the kernel
//!   rights it declares, no new privileges;
//! * readiness: a service with `notify` counts as started only when it
//!   reported `READY` within its deadline ([`supervisor`]);
//! * standard output and error into the log service, with a bounded ring
//!   while it is not there ([`output`]);
//! * restarts with back-off and a crash-loop limit, the ordered shutdown
//!   (deepest dependants first, the log service last; stop signal, the
//!   hard end after the service's deadline);
//! * the bus service `init` for administrators ([`control`], CLI `uinit`)
//!   and the text control socket of earlier releases ([`ctl`]);
//! * the loop ([`daemon`]): signals, reaping, pipes, requests.
//!
//! Works on Linux and on the Ultar kernel: what the kernel lacks (group
//! tree, rights, ids) is detected at start and reported, not assumed.

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod control;
pub mod ctl;
pub mod daemon;
pub mod groups;
pub mod launch;
pub mod msg;
pub mod order;
pub mod output;
pub mod spec;
pub mod start;
pub mod supervisor;
pub mod unit;
