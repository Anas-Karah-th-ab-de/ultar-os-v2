//! Contract of the VPN service (`ultar-vpnd`): WireGuard tunnels from imported
//! standard `.conf` files.
//!
//! The shell's quick setting `qs.vpn` and the Settings app switch the tunnel with
//! [`SetVpn`] and follow [`VpnState`]. Where the kernel has no TUN device (the Ultar
//! kernel today) [`VpnState::available`] is `false` with the reason, and every call
//! answers [`VpnOutcome::Unavailable`].

use super::auth::Secret;
use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "vpn";
/// Longest profile name (bytes).
pub const MAX_NAME: usize = 32;
/// Most profiles kept.
pub const MAX_PROFILES: usize = 16;

/// Why the VPN cannot run here.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum VpnUnavailable {
    /// The kernel offers no TUN device.
    NoTun,
    /// The service lacks the network right.
    NoRight,
}

/// One saved tunnel (no key material).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct VpnProfile {
    /// Name (`[a-z0-9_-]`, 1–32).
    pub name: String,
    /// The server, `host:port` as in the file.
    pub endpoint: String,
    /// The tunnel's own addresses, `a.b.c.d/n`.
    pub addresses: Vec<String>,
    /// All traffic goes through the tunnel (`0.0.0.0/0` allowed).
    pub full_tunnel: bool,
}

/// State of the tunnel.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VpnStatus {
    /// Off.
    #[default]
    Off,
    /// Interface up, no handshake with the server yet.
    Connecting,
    /// A handshake within the last 3 minutes.
    Up,
    /// Could not be started (interface, addresses or routes refused).
    Failed,
}

/// `vpn.state` – the VPN (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct VpnState {
    /// A tunnel can run here.
    pub available: bool,
    /// Why not.
    pub unavailable: Option<VpnUnavailable>,
    /// The saved tunnels.
    pub profiles: Vec<VpnProfile>,
    /// The tunnel switched on.
    pub active: Option<String>,
    /// Its state.
    pub status: VpnStatus,
    /// Seconds since the last handshake.
    pub handshake_age_secs: Option<u64>,
    /// Bytes received through the tunnel.
    pub rx_bytes: u64,
    /// Bytes sent through the tunnel.
    pub tx_bytes: u64,
    /// DNS servers of the active tunnel (applied by the network service).
    pub dns: Vec<String>,
}

/// `vpn.import` – save a tunnel from a WireGuard `.conf` file.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Import {
    /// Name (`[a-z0-9_-]`, 1–32).
    pub name: String,
    /// The file's text (holds the private key: never logged).
    pub config: Secret,
}

/// Why a file was refused (with the line, 1-based, 0: the whole file).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfProblem {
    /// Not text, too long, or a line too long.
    Unreadable,
    /// A line outside `[Interface]` / `[Peer]` or without `=`.
    Syntax,
    /// A key this service does not take (commands such as `PostUp` never run).
    UnknownKey,
    /// A key given twice.
    Duplicate,
    /// A key that must be there is missing.
    Missing,
    /// A key (base64, 32 bytes) is malformed.
    BadKey,
    /// An address or network is malformed.
    BadAddress,
    /// The endpoint is not `host:port`.
    BadEndpoint,
    /// A number is out of range.
    BadNumber,
    /// More than one `[Peer]` or none.
    PeerCount,
}

/// How importing went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportOutcome {
    /// Saved.
    Done,
    /// The name is malformed.
    BadName,
    /// A tunnel of that name exists.
    Exists,
    /// Too many tunnels.
    TooMany,
    /// The file was refused at this line.
    Invalid {
        /// What is wrong.
        problem: ConfProblem,
        /// Line (1-based; 0: whole file).
        line: u32,
    },
    /// No VPN here.
    Unavailable,
    /// Could not be stored.
    Failed,
}

/// Answer to [`Import`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImportReply {
    /// The result.
    pub outcome: ImportOutcome,
}

/// `vpn.set` – switch a tunnel on (the one named, else the last used) or off.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetVpn {
    /// On.
    pub on: bool,
    /// The tunnel (`None`: the last one used, or the only one).
    pub name: Option<String>,
}

/// `vpn.remove` – delete a saved tunnel (switched off first).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct RemoveVpn {
    /// Name.
    pub name: String,
}

/// How a change went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum VpnOutcome {
    /// Done.
    Done,
    /// No VPN here.
    Unavailable,
    /// No such tunnel (or none saved).
    NotFound,
    /// The kernel refused (interface, address, route).
    Failed,
}

/// Answer to [`SetVpn`] and [`RemoveVpn`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct VpnReply {
    /// The result.
    pub outcome: VpnOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    VpnUnavailable,
    VpnProfile,
    VpnStatus,
    VpnState,
    Import,
    ConfProblem,
    ImportOutcome,
    ImportReply,
    SetVpn,
    RemoveVpn,
    VpnOutcome,
    VpnReply
);

impl Topic for VpnState {
    const TOPIC: &'static str = "vpn.state";
    const RETAINED: bool = true;
}

impl Call for Import {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "import";
    type Reply = ImportReply;
}

impl Call for SetVpn {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "set";
    type Reply = VpnReply;
}

impl Call for RemoveVpn {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "remove";
    type Reply = VpnReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn round_trip_and_the_key_stays_hidden() {
        let i = Import {
            name: "office".into(),
            config: Secret::new("PrivateKey = c2VjcmV0".into()),
        };
        assert_eq!(decode::<Import>(&encode(&i).unwrap()).unwrap(), i);
        assert!(!format!("{i:?}").contains("c2VjcmV0"));
        let s = VpnState {
            available: true,
            profiles: vec![VpnProfile {
                name: "office".into(),
                endpoint: "vpn.example:51820".into(),
                addresses: vec!["10.8.0.2/32".into()],
                full_tunnel: true,
            }],
            active: Some("office".into()),
            status: VpnStatus::Up,
            handshake_age_secs: Some(4),
            ..VpnState::default()
        };
        assert_eq!(decode::<VpnState>(&encode(&s).unwrap()).unwrap(), s);
        let r = ImportReply {
            outcome: ImportOutcome::Invalid {
                problem: ConfProblem::UnknownKey,
                line: 7,
            },
        };
        assert_eq!(decode::<ImportReply>(&encode(&r).unwrap()).unwrap(), r);
        assert_eq!(<SetVpn as Call>::METHOD, "set");
        assert_eq!(<RemoveVpn as Call>::METHOD, "remove");
        assert_eq!(<VpnState as Topic>::TOPIC, "vpn.state");
    }
}
