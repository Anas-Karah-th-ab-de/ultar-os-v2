//! Wi-Fi networks and VPN connections in the quick settings (qs.wifi.list, qs.vpn,
//! qs.vpn.list; contracts `net.networks/join/disconnect/forget/known`, topic `net.wifi`,
//! `vpn.state/set`, docs/integration/system-services.md §3). The shell keeps what the network
//! service reported and decides what a click does; it never holds a password – the typed
//! one goes straight from the password field into the `net.join` call.

use ultar_bus::messages::net::{JoinOutcome, NetworkInfo, WifiLink, WifiPhase, WifiSecurity};
use ultar_bus::messages::vpn::{VpnState, VpnStatus, VpnUnavailable};

/// Most networks listed (one row per name).
pub const MAX_NETWORKS: usize = 12;
/// Most saved networks kept.
pub const MAX_KNOWN: usize = 64;
/// Longest network name shown (characters; names are at most 32 octets).
pub const MAX_SSID: usize = 32;

/// What the shell knows of Wi-Fi.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wifi {
    /// `net.wifi`: the connection (`None`: the service never reported).
    pub link: Option<WifiLink>,
    /// The access points of the last `net.networks` (`None`: not asked yet or no answer).
    pub networks: Option<Vec<NetworkInfo>>,
    /// The saved networks (`net.known`), at most [`MAX_KNOWN`].
    pub known: Vec<String>,
    /// The join dialog, while it is open.
    pub joining: Option<Joining>,
}

impl Wifi {
    /// Keeps the access points, at most four per listed network.
    pub fn set_networks(&mut self, mut networks: Vec<NetworkInfo>) {
        networks.truncate(MAX_NETWORKS * 4);
        self.networks = Some(networks);
    }

    /// Keeps the saved networks, at most [`MAX_KNOWN`].
    pub fn set_known(&mut self, mut ssids: Vec<String>) {
        ssids.truncate(MAX_KNOWN);
        self.known = ssids;
    }
}

/// The join dialog of a network that needs a password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joining {
    /// The network's name.
    pub ssid: String,
    /// Its security.
    pub security: WifiSecurity,
    /// Save it (reconnect automatically).
    pub remember: bool,
    /// The join call is running.
    pub busy: bool,
    /// How the last try ended (shown under the field).
    pub failed: Option<JoinOutcome>,
}

/// What a click on a network row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkClick {
    /// Join without asking (open, or saved).
    Join(String),
    /// Ask for the password first.
    AskPassword(String, WifiSecurity),
    /// Leave it (it is the connected one).
    Disconnect,
    /// Nothing: the network cannot be joined (the row says why).
    Nothing,
}

/// One network as listed (the strongest access point of its name).
pub fn listed(networks: &[NetworkInfo]) -> Vec<&NetworkInfo> {
    let mut out: Vec<&NetworkInfo> = Vec::new();
    for n in networks.iter().filter(|n| !n.ssid.is_empty()) {
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) if n.connected || (!o.connected && o.signal_percent < n.signal_percent) => {
                *o = n
            }
            Some(_) => {}
            None => out.push(n),
        }
    }
    out.sort_by_key(|n| (!n.connected, !n.known, std::cmp::Reverse(n.signal_percent)));
    out.truncate(MAX_NETWORKS);
    out
}

/// What a click on `ssid` does.
pub fn click(networks: &[NetworkInfo], ssid: &str) -> NetworkClick {
    let Some(n) = listed(networks).into_iter().find(|n| n.ssid == ssid) else {
        return NetworkClick::Nothing;
    };
    if n.connected {
        NetworkClick::Disconnect
    } else if !n.joinable {
        NetworkClick::Nothing
    } else if n.known || n.security == WifiSecurity::Open {
        NetworkClick::Join(n.ssid.clone())
    } else {
        NetworkClick::AskPassword(n.ssid.clone(), n.security)
    }
}

/// Catalog key of a network's security line.
pub fn security_key(n: &NetworkInfo) -> &'static str {
    if n.connected {
        return "desk.wifi.connected";
    }
    match (n.security, n.joinable) {
        (_, false) => "desk.wifi.unsupported",
        (WifiSecurity::Open, _) if n.known => "desk.wifi.saved_open",
        (WifiSecurity::Open, _) => "desk.wifi.open",
        (_, _) if n.known => "desk.wifi.saved",
        _ => "desk.wifi.secured",
    }
}

/// The signal as 0–4 bars (DESKTOP-SPEC §4.1 icons: the fan in four steps).
pub fn bars(signal_percent: u8) -> u8 {
    match signal_percent {
        0..=19 => 0,
        20..=39 => 1,
        40..=59 => 2,
        60..=79 => 3,
        _ => 4,
    }
}

/// Catalog key of the line under the password field for a join that did not connect.
pub fn join_error_key(outcome: JoinOutcome) -> &'static str {
    match outcome {
        JoinOutcome::Connected => "desk.wifi.join.connected",
        JoinOutcome::NoDevice => "desk.wifi.join.no_device",
        JoinOutcome::RadioOff => "desk.wifi.join.radio_off",
        JoinOutcome::NotFound => "desk.wifi.join.not_found",
        JoinOutcome::PasswordNeeded => "desk.wifi.join.password_needed",
        JoinOutcome::BadPassword => "desk.wifi.join.bad_password",
        JoinOutcome::WrongPassword => "desk.wifi.join.wrong_password",
        JoinOutcome::Unsupported => "desk.wifi.join.unsupported",
        JoinOutcome::Refused => "desk.wifi.join.refused",
        JoinOutcome::Timeout => "desk.wifi.join.timeout",
        JoinOutcome::Busy => "desk.wifi.join.busy",
        JoinOutcome::Failed => "desk.wifi.join.failed",
    }
}

/// The connected network's name for the Wi-Fi tile (board: "KM-Office").
pub fn connected_ssid(link: Option<&WifiLink>) -> Option<String> {
    let link = link?;
    let shown = matches!(
        link.phase,
        WifiPhase::Connected | WifiPhase::ObtainingAddress
    );
    shown
        .then_some(link.ssid.as_ref())
        .flatten()
        .map(|s| s.chars().take(MAX_SSID).collect())
}

/// Catalog key of the Wi-Fi tile's line while it is on but not connected.
pub fn phase_key(phase: WifiPhase) -> Option<&'static str> {
    match phase {
        WifiPhase::Scanning => Some("desk.wifi.phase.scanning"),
        WifiPhase::Associating | WifiPhase::Authenticating => Some("desk.wifi.phase.joining"),
        WifiPhase::ObtainingAddress => Some("desk.wifi.phase.address"),
        WifiPhase::Disconnected => Some("desk.wifi.phase.disconnected"),
        _ => None,
    }
}

/// The VPN is on (up or connecting).
pub fn vpn_on(vpn: &VpnState) -> bool {
    matches!(vpn.status, VpnStatus::Up | VpnStatus::Connecting)
}

/// Why the VPN cannot be used (catalog key), or `None`.
pub fn vpn_reason(vpn: &VpnState) -> Option<&'static str> {
    if vpn.available {
        return None;
    }
    Some(match vpn.unavailable {
        Some(VpnUnavailable::NoTun) => "desk.reason.vpn_no_tun",
        Some(VpnUnavailable::NoRight) | None => "desk.reason.vpn_no_right", // physical: a right, not a side
    })
}

/// Catalog key of the VPN tile's state line.
pub fn vpn_key(vpn: &VpnState) -> &'static str {
    match vpn.status {
        VpnStatus::Up => "desk.vpn.up",
        VpnStatus::Connecting => "desk.vpn.connecting",
        VpnStatus::Failed => "desk.vpn.failed",
        VpnStatus::Off if vpn.profiles.is_empty() => "desk.vpn.none",
        VpnStatus::Off => "desk.state.off",
    }
}

#[cfg(test)]
#[path = "wifi_tests.rs"]
mod tests;
