//! Reading nl80211 answers and events (hostile-safe: missing or short
//! attributes make the item absent, never a panic).

use super::{attr, cmd, Bss, WifiEvent, WifiInterface, IFTYPE_STATION};
use crate::attr::{find, get_mac, get_string, get_u16, get_u32};
use crate::genl::split;

const BSS_BSSID: u16 = 1;
const BSS_FREQUENCY: u16 = 2;
const BSS_CAPABILITY: u16 = 5;
const BSS_INFORMATION_ELEMENTS: u16 = 6;
const BSS_SIGNAL_MBM: u16 = 7;
const BSS_BEACON_IES: u16 = 11;
const STA_INFO_SIGNAL: u16 = 7;
/// Longest element list taken from one BSS.
const MAX_IES: usize = 4096;

/// A station interface of a `GET_INTERFACE` answer.
pub fn parse_interface(payload: &[u8]) -> Option<WifiInterface> {
    let (_, a) = split(payload).ok()?;
    if get_u32(a, attr::IFTYPE)? != IFTYPE_STATION {
        return None;
    }
    Some(WifiInterface {
        index: get_u32(a, attr::IFINDEX)?,
        name: get_string(a, attr::IFNAME)?,
        mac: get_mac(a, attr::MAC)?,
    })
}

/// A network of a `GET_SCAN` answer.
pub fn parse_bss(payload: &[u8]) -> Option<Bss> {
    let (_, a) = split(payload).ok()?;
    let b = find(a, attr::BSS)?;
    let ies = find(b, BSS_INFORMATION_ELEMENTS)
        .or_else(|| find(b, BSS_BEACON_IES))
        .unwrap_or(&[]);
    if ies.len() > MAX_IES {
        return None;
    }
    Some(Bss {
        bssid: get_mac(b, BSS_BSSID)?,
        freq_mhz: get_u32(b, BSS_FREQUENCY)?,
        signal_mbm: get_u32(b, BSS_SIGNAL_MBM).map_or(-10_000, |v| v as i32),
        capability: get_u16(b, BSS_CAPABILITY).unwrap_or(0),
        ies: ies.to_vec(),
    })
}

/// The signal (dBm) of a `GET_STATION` answer.
pub fn station_signal(payload: &[u8]) -> Option<i8> {
    let (_, a) = split(payload).ok()?;
    let info = find(a, attr::STA_INFO)?;
    find(info, STA_INFO_SIGNAL)
        .and_then(|v| v.first())
        .map(|b| *b as i8)
}

/// An event the service follows (others: `None`).
pub fn parse_event(payload: &[u8]) -> Option<WifiEvent> {
    let (command, a) = split(payload).ok()?;
    let ifindex = get_u32(a, attr::IFINDEX)?;
    Some(match command {
        cmd::NEW_SCAN_RESULTS => WifiEvent::ScanDone(ifindex),
        cmd::SCAN_ABORTED => WifiEvent::ScanAborted(ifindex),
        cmd::CONNECT => WifiEvent::Connected {
            ifindex,
            bssid: get_mac(a, attr::MAC),
            status: if find(a, attr::TIMED_OUT).is_some() {
                u16::MAX
            } else {
                get_u16(a, attr::STATUS_CODE).unwrap_or(1)
            },
        },
        cmd::DISCONNECT => WifiEvent::Disconnected {
            ifindex,
            reason: get_u16(a, attr::REASON_CODE).unwrap_or(0),
        },
        cmd::CONTROL_PORT_FRAME => {
            if get_u16(a, attr::CONTROL_PORT_ETHERTYPE) != Some(super::ETH_P_PAE) {
                return None;
            }
            WifiEvent::Eapol {
                ifindex,
                from: get_mac(a, attr::MAC)?,
                frame: find(a, attr::FRAME)?.to_vec(),
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr::{messages, Builder};

    fn genl(command: u8, add: impl FnOnce(Builder) -> Builder) -> Vec<u8> {
        let bytes = add(Builder::new(30, 0).raw(&[command, 1, 0, 0])).finish(0);
        let m = messages(&bytes).next().unwrap().unwrap();
        m.payload.to_vec()
    }

    #[test]
    fn interfaces_networks_and_signal() {
        let p = genl(7, |b| {
            b.u32(attr::IFINDEX, 4)
                .string(attr::IFNAME, "wlan0")
                .u32(attr::IFTYPE, IFTYPE_STATION)
                .attr(attr::MAC, &[2, 1, 1, 1, 1, 1])
        });
        assert_eq!(
            parse_interface(&p),
            Some(WifiInterface {
                index: 4,
                name: "wlan0".into(),
                mac: [2, 1, 1, 1, 1, 1]
            })
        );
        let ap = genl(7, |b| b.u32(attr::IFINDEX, 5).u32(attr::IFTYPE, 3));
        assert_eq!(parse_interface(&ap), None);
        let p = genl(34, |b| {
            b.u32(attr::IFINDEX, 4).nest(attr::BSS, |b| {
                b.attr(BSS_BSSID, &[2, 9, 9, 9, 9, 9])
                    .u32(BSS_FREQUENCY, 2412)
                    .u16(BSS_CAPABILITY, 0x11)
                    .attr(BSS_INFORMATION_ELEMENTS, &[0, 2, b'h', b'i'])
                    .u32(BSS_SIGNAL_MBM, (-5500i32) as u32)
            })
        });
        let bss = parse_bss(&p).unwrap();
        assert_eq!(
            (bss.freq_mhz, bss.signal_mbm, bss.capability),
            (2412, -5500, 0x11)
        );
        assert_eq!(bss.ies, vec![0, 2, b'h', b'i']);
        assert_eq!(parse_bss(&genl(34, |b| b.u32(attr::IFINDEX, 4))), None);
        let p = genl(17, |b| {
            b.nest(attr::STA_INFO, |b| b.u8(STA_INFO_SIGNAL, (-61i8) as u8))
        });
        assert_eq!(station_signal(&p), Some(-61));
    }

    #[test]
    fn events() {
        let e = |c, add: fn(Builder) -> Builder| {
            parse_event(&genl(c, |b| add(b.u32(attr::IFINDEX, 4))))
        };
        assert_eq!(
            e(cmd::NEW_SCAN_RESULTS, |b| b),
            Some(WifiEvent::ScanDone(4))
        );
        assert_eq!(e(cmd::SCAN_ABORTED, |b| b), Some(WifiEvent::ScanAborted(4)));
        assert_eq!(
            e(cmd::CONNECT, |b| b
                .u16(attr::STATUS_CODE, 0)
                .attr(attr::MAC, &[1; 6])),
            Some(WifiEvent::Connected {
                ifindex: 4,
                bssid: Some([1; 6]),
                status: 0
            })
        );
        assert_eq!(
            e(cmd::CONNECT, |b| b.flag(attr::TIMED_OUT)),
            Some(WifiEvent::Connected {
                ifindex: 4,
                bssid: None,
                status: u16::MAX
            })
        );
        assert_eq!(
            e(cmd::DISCONNECT, |b| b.u16(attr::REASON_CODE, 15)),
            Some(WifiEvent::Disconnected {
                ifindex: 4,
                reason: 15
            })
        );
        assert_eq!(
            e(cmd::CONTROL_PORT_FRAME, |b| b
                .u16(attr::CONTROL_PORT_ETHERTYPE, 0x888e)
                .attr(attr::MAC, &[3; 6])
                .attr(attr::FRAME, &[2, 3, 0, 0])),
            Some(WifiEvent::Eapol {
                ifindex: 4,
                from: [3; 6],
                frame: vec![2, 3, 0, 0]
            })
        );
        assert_eq!(
            e(cmd::CONTROL_PORT_FRAME, |b| b
                .u16(attr::CONTROL_PORT_ETHERTYPE, 0x0800)),
            None
        );
        assert_eq!(e(99, |b| b), None);
        assert_eq!(parse_event(&[1, 2]), None);
    }
}
