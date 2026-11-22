//! ultar-ril – Transport + Protokoll zum Shannon-Modem (SS310AP) des A30.
//! Schichten: sipc5 (Rahmen-Doku/Tests), link (ModemLink/UmtsLink über /dev/umts_ipc0),
//! ipc (Samsung-IPC-FMT-Nachricht), sim/net (SIM/Netz-Befehle), pdp (Daten→rmnet).
//! Voraussetzung: CP gebootet+ONLINE (crates/ultar-modem).

pub mod link;
pub mod sipc5;
pub mod tel;
pub mod ipc;
pub mod sim;
pub mod net;
pub mod pdp;
pub mod mock;
pub mod rfs;
pub mod engine;

pub use link::{channel_to_node, MockLink, ModemLink, SipcFrame, UmtsLink, CH_FMT_0, CH_FMT_1, CH_PDP_0, CH_RFS_0, CH_ROUTER};
