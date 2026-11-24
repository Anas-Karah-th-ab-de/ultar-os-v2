//! nl80211, the kernel's Wi-Fi interface: station interfaces, scanning,
//! connecting with the supplicant in user space (keys installed by this
//! service, EAPOL frames over the control port of nl80211, so no raw packet
//! socket is needed), the port opened after the handshake, link signal.
//!
//! Constants are those of `linux/nl80211.h` (checked against the header).

mod parse;

pub use parse::{parse_bss, parse_event, parse_interface};

use crate::attr::{Builder, F_DUMP, F_REQUEST};
use crate::genl::{self, Family};
use crate::socket::{Socket, CHANGE, GENERIC};
use crate::NetError;
use std::time::Duration;

/// Commands.
pub mod cmd {
    /// List interfaces.
    pub const GET_INTERFACE: u8 = 5;
    /// Install a key.
    pub const NEW_KEY: u8 = 11;
    /// Station info.
    pub const GET_STATION: u8 = 17;
    /// Change a station (open the port).
    pub const SET_STATION: u8 = 18;
    /// Scan results.
    pub const GET_SCAN: u8 = 32;
    /// Start a scan.
    pub const TRIGGER_SCAN: u8 = 33;
    /// Event: scan results ready.
    pub const NEW_SCAN_RESULTS: u8 = 34;
    /// Event: scan aborted.
    pub const SCAN_ABORTED: u8 = 35;
    /// Connect / event: connect result.
    pub const CONNECT: u8 = 46;
    /// Disconnect / event: disconnected.
    pub const DISCONNECT: u8 = 48;
    /// EAPOL frame over the control port.
    pub const CONTROL_PORT_FRAME: u8 = 129;
}

/// Attributes.
pub mod attr {
    /// Wiphy index.
    pub const WIPHY: u16 = 1;
    /// Interface index.
    pub const IFINDEX: u16 = 3;
    /// Interface name.
    pub const IFNAME: u16 = 4;
    /// Interface type.
    pub const IFTYPE: u16 = 5;
    /// Hardware address.
    pub const MAC: u16 = 6;
    /// Key data.
    pub const KEY_DATA: u16 = 7;
    /// Key index.
    pub const KEY_IDX: u16 = 8;
    /// Key cipher.
    pub const KEY_CIPHER: u16 = 9;
    /// Key sequence counter.
    pub const KEY_SEQ: u16 = 10;
    /// Station info (nested).
    pub const STA_INFO: u16 = 21;
    /// Frequency (MHz).
    pub const WIPHY_FREQ: u16 = 38;
    /// Information elements.
    pub const IE: u16 = 42;
    /// SSIDs to scan (nested).
    pub const SCAN_SSIDS: u16 = 45;
    /// A BSS (nested).
    pub const BSS: u16 = 47;
    /// EAPOL frame.
    pub const FRAME: u16 = 51;
    /// SSID.
    pub const SSID: u16 = 52;
    /// Authentication type.
    pub const AUTH_TYPE: u16 = 53;
    /// Reason code.
    pub const REASON_CODE: u16 = 54;
    /// Key type.
    pub const KEY_TYPE: u16 = 55;
    /// Connect timed out.
    pub const TIMED_OUT: u16 = 65;
    /// Station flags (mask, set).
    pub const STA_FLAGS2: u16 = 67;
    /// Control port.
    pub const CONTROL_PORT: u16 = 68;
    /// Privacy.
    pub const PRIVACY: u16 = 70;
    /// Disconnected by the access point.
    pub const DISCONNECTED_BY_AP: u16 = 71;
    /// Status code.
    pub const STATUS_CODE: u16 = 72;
    /// Pairwise ciphers.
    pub const CIPHER_SUITES_PAIRWISE: u16 = 73;
    /// Group cipher.
    pub const CIPHER_SUITE_GROUP: u16 = 74;
    /// WPA versions.
    pub const WPA_VERSIONS: u16 = 75;
    /// Key management suites.
    pub const AKM_SUITES: u16 = 76;
    /// Control port ethertype.
    pub const CONTROL_PORT_ETHERTYPE: u16 = 102;
    /// Send the control port frame unencrypted.
    pub const CONTROL_PORT_NO_ENCRYPT: u16 = 103;
    /// The connection belongs to this socket (ends when it closes).
    pub const SOCKET_OWNER: u16 = 204;
    /// EAPOL over nl80211 instead of the network interface.
    pub const CONTROL_PORT_OVER_NL80211: u16 = 264;
}

/// Station interface type.
pub const IFTYPE_STATION: u32 = 2;
/// CCMP-128 cipher suite.
pub const CIPHER_CCMP: u32 = 0x000f_ac04;
/// PSK key management suite.
pub const AKM_PSK: u32 = 0x000f_ac02;
/// EAPOL ethertype.
pub const ETH_P_PAE: u16 = 0x888e;
/// Station flag: authorized.
const STA_FLAG_AUTHORIZED: u32 = 1;
/// Key types.
const KEYTYPE_GROUP: u32 = 0;
const KEYTYPE_PAIRWISE: u32 = 1;

/// A Wi-Fi station interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiInterface {
    /// Interface index.
    pub index: u32,
    /// Interface name.
    pub name: String,
    /// Hardware address.
    pub mac: [u8; 6],
}

/// One network seen by a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bss {
    /// Its address.
    pub bssid: [u8; 6],
    /// Frequency (MHz).
    pub freq_mhz: u32,
    /// Signal (1/100 dBm).
    pub signal_mbm: i32,
    /// Capability field (bit 4: privacy).
    pub capability: u16,
    /// Information elements (probe response, else beacon).
    pub ies: Vec<u8>,
}

/// What to connect to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectParams {
    /// SSID.
    pub ssid: Vec<u8>,
    /// The access point.
    pub bssid: [u8; 6],
    /// Its frequency.
    pub freq_mhz: u32,
    /// WPA2-Personal: the own RSN element (`None`: open network).
    pub rsn: Option<Vec<u8>>,
}

/// Something the kernel reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WifiEvent {
    /// Scan results are ready.
    ScanDone(u32),
    /// The scan was aborted.
    ScanAborted(u32),
    /// Result of a connect (status 0: associated).
    Connected {
        /// Interface.
        ifindex: u32,
        /// Access point.
        bssid: Option<[u8; 6]>,
        /// IEEE status code (0: success).
        status: u16,
    },
    /// The connection ended.
    Disconnected {
        /// Interface.
        ifindex: u32,
        /// IEEE reason code.
        reason: u16,
    },
    /// An EAPOL frame from the access point.
    Eapol {
        /// Interface.
        ifindex: u32,
        /// Sender.
        from: [u8; 6],
        /// The frame (EAPOL header first).
        frame: Vec<u8>,
    },
}

/// The nl80211 family on its own socket.
#[derive(Debug)]
pub struct Nl80211 {
    socket: Socket,
    family: Family,
}

impl Nl80211 {
    /// Opens nl80211 and joins its `mlme` and `scan` events;
    /// [`NetError::Unavailable`] when the kernel has no Wi-Fi support.
    pub fn open() -> Result<Nl80211, NetError> {
        let mut socket = Socket::open(GENERIC)?;
        let family = genl::resolve(&mut socket, "nl80211")?;
        for group in ["mlme", "scan"] {
            if let Some(id) = family.group(group) {
                socket.join(id)?;
            }
        }
        Ok(Nl80211 { socket, family })
    }

    /// The descriptor (to wait for events).
    pub fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.socket.fd()
    }

    fn msg(&self, cmd: u8, flags: u16) -> Builder {
        genl::header(self.family.id, cmd, flags)
    }

    /// The station interfaces.
    pub fn interfaces(&mut self) -> Result<Vec<WifiInterface>, NetError> {
        let answers = self
            .socket
            .request(self.msg(cmd::GET_INTERFACE, F_REQUEST | F_DUMP))?;
        Ok(answers.iter().filter_map(|a| parse_interface(a)).collect())
    }

    /// Starts a scan of all channels for all networks.
    pub fn trigger_scan(&mut self, ifindex: u32) -> Result<(), NetError> {
        let m = self
            .msg(cmd::TRIGGER_SCAN, CHANGE)
            .u32(attr::IFINDEX, ifindex)
            .nest(attr::SCAN_SSIDS, |b| b.attr(1, &[]));
        self.socket.change(m)
    }

    /// The networks of the last scan.
    pub fn scan_results(&mut self, ifindex: u32) -> Result<Vec<Bss>, NetError> {
        let m = self
            .msg(cmd::GET_SCAN, F_REQUEST | F_DUMP)
            .u32(attr::IFINDEX, ifindex);
        let answers = self.socket.request(m)?;
        Ok(answers.iter().filter_map(|a| parse_bss(a)).collect())
    }

    /// Connects (association; the handshake follows over the control port).
    pub fn connect(&mut self, ifindex: u32, p: &ConnectParams) -> Result<(), NetError> {
        let mut m = self
            .msg(cmd::CONNECT, CHANGE)
            .u32(attr::IFINDEX, ifindex)
            .attr(attr::SSID, &p.ssid)
            .attr(attr::MAC, &p.bssid)
            .u32(attr::WIPHY_FREQ, p.freq_mhz)
            .u32(attr::AUTH_TYPE, 0)
            .flag(attr::SOCKET_OWNER);
        if let Some(rsn) = &p.rsn {
            m = m
                .flag(attr::PRIVACY)
                .u32(attr::WPA_VERSIONS, 2)
                .u32(attr::CIPHER_SUITES_PAIRWISE, CIPHER_CCMP)
                .u32(attr::CIPHER_SUITE_GROUP, CIPHER_CCMP)
                .u32(attr::AKM_SUITES, AKM_PSK)
                .attr(attr::IE, rsn)
                .flag(attr::CONTROL_PORT)
                .u16(attr::CONTROL_PORT_ETHERTYPE, ETH_P_PAE)
                .flag(attr::CONTROL_PORT_OVER_NL80211);
        }
        self.socket.change(m)
    }

    /// Disconnects (reason 3: leaving).
    pub fn disconnect(&mut self, ifindex: u32) -> Result<(), NetError> {
        let m = self
            .msg(cmd::DISCONNECT, CHANGE)
            .u32(attr::IFINDEX, ifindex)
            .u16(attr::REASON_CODE, 3);
        self.socket.change(m)
    }

    /// Sends an EAPOL frame to the access point (unencrypted until the
    /// pairwise key is installed).
    pub fn send_eapol(
        &mut self,
        ifindex: u32,
        to: [u8; 6],
        frame: &[u8],
        encrypt: bool,
    ) -> Result<(), NetError> {
        let mut m = self
            .msg(cmd::CONTROL_PORT_FRAME, CHANGE)
            .u32(attr::IFINDEX, ifindex)
            .attr(attr::FRAME, frame)
            .attr(attr::MAC, &to)
            .u16(attr::CONTROL_PORT_ETHERTYPE, ETH_P_PAE);
        if !encrypt {
            m = m.flag(attr::CONTROL_PORT_NO_ENCRYPT);
        }
        self.socket.change(m)
    }

    /// Installs a CCMP key: pairwise (`peer` given, index 0) or group.
    pub fn install_key(
        &mut self,
        ifindex: u32,
        peer: Option<[u8; 6]>,
        index: u8,
        key: &[u8],
        rsc: &[u8; 8],
    ) -> Result<(), NetError> {
        let mut m = self
            .msg(cmd::NEW_KEY, CHANGE)
            .u32(attr::IFINDEX, ifindex)
            .attr(attr::KEY_DATA, key)
            .u8(attr::KEY_IDX, index)
            .u32(attr::KEY_CIPHER, CIPHER_CCMP);
        m = match peer {
            Some(mac) => m
                .attr(attr::MAC, &mac)
                .u32(attr::KEY_TYPE, KEYTYPE_PAIRWISE),
            None => m
                .attr(attr::KEY_SEQ, &rsc[..6])
                .u32(attr::KEY_TYPE, KEYTYPE_GROUP),
        };
        self.socket.change(m)
    }

    /// Opens the port for data after the handshake.
    pub fn authorize(&mut self, ifindex: u32, peer: [u8; 6]) -> Result<(), NetError> {
        let mut flags = [0u8; 8];
        flags[..4].copy_from_slice(&(1u32 << STA_FLAG_AUTHORIZED).to_ne_bytes());
        flags[4..].copy_from_slice(&(1u32 << STA_FLAG_AUTHORIZED).to_ne_bytes());
        let m = self
            .msg(cmd::SET_STATION, CHANGE)
            .u32(attr::IFINDEX, ifindex)
            .attr(attr::MAC, &peer)
            .attr(attr::STA_FLAGS2, &flags);
        self.socket.change(m)
    }

    /// The signal of the access point (dBm), when the driver reports it.
    pub fn signal(&mut self, ifindex: u32, peer: [u8; 6]) -> Result<Option<i8>, NetError> {
        let m = self
            .msg(cmd::GET_STATION, CHANGE)
            .u32(attr::IFINDEX, ifindex)
            .attr(attr::MAC, &peer);
        let answers = self.socket.request(m)?;
        Ok(answers.first().and_then(|a| parse::station_signal(a)))
    }

    /// The next event, waiting at most `wait`.
    pub fn next_event(&mut self, wait: Option<Duration>) -> Result<Option<WifiEvent>, NetError> {
        while let Some(e) = self.socket.next_event(wait)? {
            if e.kind != self.family.id {
                continue;
            }
            if let Some(event) = parse_event(&e.payload) {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
}

/// Signal in 1/100 dBm as 0–100 % (−90 dBm and below 0, −30 dBm and above 100).
pub fn signal_percent(mbm: i32) -> u8 {
    let dbm = mbm / 100;
    ((dbm.clamp(-90, -30) + 90) * 100 / 60) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_scale() {
        assert_eq!(signal_percent(-9000), 0);
        assert_eq!(signal_percent(-12000), 0);
        assert_eq!(signal_percent(-3000), 100);
        assert_eq!(signal_percent(-6000), 50);
    }

    #[test]
    fn a_kernel_without_wifi_says_unavailable() {
        // This host's kernel has no cfg80211: the family is missing.
        match Nl80211::open() {
            Err(NetError::Unavailable) => {}
            Ok(mut n) => {
                let _ = n.interfaces().unwrap();
            }
            Err(e) => panic!("{e:?}"),
        }
    }
}
