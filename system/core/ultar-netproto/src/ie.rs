//! Information elements of a Wi-Fi scan result (IEEE 802.11-2016 9.4.2):
//! the network name and what security it asks for.

use crate::wpa::rsn::{Akm, Cipher, RsnInfo};

/// Element id of the SSID.
pub const SSID: u8 = 0;
/// Element id of the RSN element.
pub const RSN: u8 = 48;
/// Element id of vendor elements.
pub const VENDOR: u8 = 221;
/// Longest SSID (bytes).
pub const MAX_SSID: usize = 32;

/// The elements of a frame body, in order; stops at the first element that
/// does not fit (hostile input ends the walk, it never reads past the end).
pub fn elements(mut data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    std::iter::from_fn(move || {
        let (&id, rest) = data.split_first()?;
        let (&len, rest) = rest.split_first()?;
        let len = usize::from(len);
        if rest.len() < len {
            data = &[];
            return None;
        }
        let (body, tail) = rest.split_at(len);
        data = tail;
        Some((id, body))
    })
}

/// The first element with `id`, whole (id and length bytes included).
pub fn find_raw(data: &[u8], id: u8) -> Option<Vec<u8>> {
    elements(data).find(|(i, _)| *i == id).map(|(i, body)| {
        let mut raw = Vec::with_capacity(body.len() + 2);
        raw.push(i);
        raw.push(body.len() as u8);
        raw.extend_from_slice(body);
        raw
    })
}

/// What security a network asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// No password.
    Open,
    /// WPA2-Personal (pre-shared key, CCMP).
    Wpa2Personal,
    /// WPA3-Personal transition mode: WPA2-Personal is offered too.
    Wpa2Wpa3Personal,
    /// WPA3-Personal only (SAE).
    Wpa3Personal,
    /// WPA2/WPA3-Enterprise (802.1X).
    Enterprise,
    /// WEP or WPA (version 1): broken, never joined.
    Legacy,
    /// An RSN element this release cannot use (other ciphers, required
    /// management frame protection with WPA2-Personal only, …).
    Other,
}

impl Security {
    /// This release can join it (open or WPA2-Personal with CCMP).
    pub fn joinable(self) -> bool {
        matches!(
            self,
            Security::Open | Security::Wpa2Personal | Security::Wpa2Wpa3Personal
        )
    }

    /// A password is needed.
    pub fn secured(self) -> bool {
        self != Security::Open
    }
}

/// The SSID bytes of a scan result (`None`: missing or longer than 32).
pub fn ssid(data: &[u8]) -> Option<Vec<u8>> {
    elements(data)
        .find(|(id, _)| *id == SSID)
        .filter(|(_, body)| body.len() <= MAX_SSID)
        .map(|(_, body)| body.to_vec())
}

fn wpa1(data: &[u8]) -> bool {
    elements(data).any(|(id, body)| id == VENDOR && body.starts_with(&[0x00, 0x50, 0xf2, 0x01]))
}

/// The security of a network from its elements and the privacy bit of its
/// capability field.
pub fn security(data: &[u8], privacy: bool) -> Security {
    let Some((_, body)) = elements(data).find(|(id, _)| *id == RSN) else {
        return if privacy || wpa1(data) {
            Security::Legacy
        } else {
            Security::Open
        };
    };
    let Ok(rsn) = RsnInfo::parse_body(body) else {
        return Security::Other;
    };
    classify(&rsn)
}

fn classify(rsn: &RsnInfo) -> Security {
    let ccmp = rsn.pairwise.contains(&Cipher::Ccmp) && rsn.group == Cipher::Ccmp;
    let psk = rsn.akms.contains(&Akm::Psk);
    let sae = rsn.akms.contains(&Akm::Sae);
    let enterprise = rsn.akms.iter().any(|a| matches!(a, Akm::Ieee8021x));
    match (psk, sae) {
        // WPA2-Personal without management frame protection (MFPR = 0).
        (true, _) if ccmp && !rsn.mfp_required() => {
            if sae {
                Security::Wpa2Wpa3Personal
            } else {
                Security::Wpa2Personal
            }
        }
        (false, true) => Security::Wpa3Personal,
        _ if enterprise => Security::Enterprise,
        _ => Security::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_elements_and_stops_at_a_cut_one() {
        let data = [0, 3, b'a', b'b', b'c', 1, 2, 9, 9, 48, 40, 1];
        let list: Vec<_> = elements(&data).collect();
        assert_eq!(list, vec![(0, &b"abc"[..]), (1, &[9u8, 9][..])]);
        assert_eq!(ssid(&data), Some(b"abc".to_vec()));
        assert_eq!(find_raw(&data, 1), Some(vec![1, 2, 9, 9]));
        assert_eq!(find_raw(&data, 48), None);
        assert_eq!(
            ssid(
                &[0, 33]
                    .iter()
                    .chain([0u8; 33].iter())
                    .copied()
                    .collect::<Vec<_>>()
            ),
            None
        );
        assert_eq!(elements(&[]).count(), 0);
        assert_eq!(elements(&[7]).count(), 0);
    }

    fn rsn(pairwise: &[u8], akms: &[u8], caps: u16) -> Vec<u8> {
        let mut b = vec![1, 0, 0x00, 0x0f, 0xac, 4, pairwise.len() as u8, 0];
        for p in pairwise {
            b.extend_from_slice(&[0x00, 0x0f, 0xac, *p]);
        }
        b.extend_from_slice(&[akms.len() as u8, 0]);
        for a in akms {
            b.extend_from_slice(&[0x00, 0x0f, 0xac, *a]);
        }
        b.extend_from_slice(&caps.to_le_bytes());
        let mut e = vec![48, b.len() as u8];
        e.extend(b);
        e
    }

    #[test]
    fn security_kinds() {
        assert_eq!(security(&[0, 1, b'x'], false), Security::Open);
        assert_eq!(security(&[0, 1, b'x'], true), Security::Legacy);
        assert_eq!(
            security(&[221, 4, 0x00, 0x50, 0xf2, 0x01], false),
            Security::Legacy
        );
        assert_eq!(security(&rsn(&[4], &[2], 0), true), Security::Wpa2Personal);
        assert_eq!(
            security(&rsn(&[4], &[2, 8], 0x80), true),
            Security::Wpa2Wpa3Personal
        );
        assert_eq!(
            security(&rsn(&[4], &[8], 0xc0), true),
            Security::Wpa3Personal
        );
        assert_eq!(security(&rsn(&[4], &[1], 0), true), Security::Enterprise);
        // PSK with required protection of management frames: not this release.
        assert_eq!(security(&rsn(&[4], &[2], 0xc0), true), Security::Other);
        // TKIP only.
        assert_eq!(security(&rsn(&[2], &[2], 0), true), Security::Other);
        assert_eq!(security(&[48, 1, 1], true), Security::Other);
        assert!(Security::Wpa2Personal.joinable() && !Security::Wpa3Personal.joinable());
        assert!(!Security::Open.secured() && Security::Legacy.secured());
    }
}
