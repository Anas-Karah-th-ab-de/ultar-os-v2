//! The RSN element (IEEE 802.11-2016 9.4.2.25): what the network offers and
//! what this supplicant asks for (WPA2-Personal: CCMP pairwise and group,
//! AKM 00-0F-AC:2, no management frame protection).

use super::WpaError;

/// The IEEE 802.11 suite selector prefix.
pub const OUI: [u8; 3] = [0x00, 0x0f, 0xac];
/// RSN capabilities: management frame protection required.
pub const MFPR: u16 = 1 << 6;
/// RSN capabilities: management frame protection capable.
pub const MFPC: u16 = 1 << 7;

/// A cipher suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cipher {
    /// CCMP-128 (00-0F-AC:4).
    Ccmp,
    /// TKIP (00-0F-AC:2).
    Tkip,
    /// Any other suite (type byte, or 0xff for another OUI).
    Other(u8),
}

/// An authentication and key management suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Akm {
    /// 802.1X (enterprise; types 1, 3, 5, 11, 12, 13).
    Ieee8021x,
    /// Pre-shared key (00-0F-AC:2).
    Psk,
    /// Pre-shared key with SHA-256 (00-0F-AC:6).
    PskSha256,
    /// SAE (WPA3-Personal; types 8, 9, 24, 25).
    Sae,
    /// Any other suite.
    Other(u8),
}

/// A parsed RSN element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RsnInfo {
    /// Group data cipher.
    pub group: Cipher,
    /// Pairwise ciphers.
    pub pairwise: Vec<Cipher>,
    /// Key management suites.
    pub akms: Vec<Akm>,
    /// RSN capabilities.
    pub caps: u16,
}

fn suite(b: &[u8]) -> Option<u8> {
    (b[..3] == OUI).then_some(b[3])
}

fn cipher(b: &[u8]) -> Cipher {
    match suite(b) {
        Some(4) => Cipher::Ccmp,
        Some(2) => Cipher::Tkip,
        Some(t) => Cipher::Other(t),
        None => Cipher::Other(0xff),
    }
}

fn akm(b: &[u8]) -> Akm {
    match suite(b) {
        Some(1 | 3 | 5 | 11 | 12 | 13) => Akm::Ieee8021x,
        Some(2) => Akm::Psk,
        Some(6) => Akm::PskSha256,
        Some(8 | 9 | 24 | 25) => Akm::Sae,
        Some(t) => Akm::Other(t),
        None => Akm::Other(0xff),
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<Option<&'a [u8]>, WpaError> {
        if self.0.is_empty() {
            return Ok(None);
        }
        if self.0.len() < n {
            return Err(WpaError::BadElement);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(Some(a))
    }

    fn list<T>(&mut self, map: fn(&[u8]) -> T) -> Result<Option<Vec<T>>, WpaError> {
        let Some(count) = self.take(2)? else {
            return Ok(None);
        };
        let count = usize::from(u16::from_le_bytes([count[0], count[1]]));
        if count == 0 || count > 16 {
            return Err(WpaError::BadElement);
        }
        let body = self.take(count * 4)?.ok_or(WpaError::BadElement)?;
        Ok(Some(body.chunks_exact(4).map(map).collect()))
    }
}

impl RsnInfo {
    /// Parses the body of an RSN element (after id and length). Missing
    /// trailing fields take the defaults of the standard.
    pub fn parse_body(body: &[u8]) -> Result<RsnInfo, WpaError> {
        let mut r = Reader(body);
        if r.take(2)? != Some(&[1, 0][..]) {
            return Err(WpaError::BadElement);
        }
        let group = r.take(4)?.map_or(Cipher::Ccmp, cipher);
        let pairwise = r.list(cipher)?.unwrap_or_else(|| vec![Cipher::Ccmp]);
        let akms = r.list(akm)?.unwrap_or_else(|| vec![Akm::Ieee8021x]);
        let caps = r.take(2)?.map_or(0, |c| u16::from_le_bytes([c[0], c[1]]));
        Ok(RsnInfo {
            group,
            pairwise,
            akms,
            caps,
        })
    }

    /// Parses a whole RSN element (id 48 and length first).
    pub fn parse(element: &[u8]) -> Result<RsnInfo, WpaError> {
        match element {
            [48, len, body @ ..] if usize::from(*len) == body.len() => Self::parse_body(body),
            _ => Err(WpaError::BadElement),
        }
    }

    /// Management frame protection is required.
    pub fn mfp_required(&self) -> bool {
        self.caps & MFPR != 0
    }

    /// WPA2-Personal as this supplicant does it is possible with this
    /// network.
    pub fn wpa2_personal(&self) -> bool {
        self.group == Cipher::Ccmp
            && self.pairwise.contains(&Cipher::Ccmp)
            && self.akms.contains(&Akm::Psk)
            && !self.mfp_required()
    }
}

/// The RSN element this supplicant sends: CCMP group and pairwise, PSK, no
/// capabilities.
pub fn supplicant_element() -> Vec<u8> {
    let mut e = vec![48, 20, 1, 0];
    e.extend_from_slice(&OUI);
    e.push(4);
    e.extend_from_slice(&[1, 0]);
    e.extend_from_slice(&OUI);
    e.push(4);
    e.extend_from_slice(&[1, 0]);
    e.extend_from_slice(&OUI);
    e.push(2);
    e.extend_from_slice(&[0, 0]);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_offers_and_builds_the_own_element() {
        let ap = [
            0x30, 0x18, 1, 0, 0, 0x0f, 0xac, 4, 2, 0, 0, 0x0f, 0xac, 4, 0, 0x0f, 0xac, 2, 1, 0, 0,
            0x0f, 0xac, 2, 0x0c, 0,
        ];
        let info = RsnInfo::parse(&ap).unwrap();
        assert_eq!(info.group, Cipher::Ccmp);
        assert_eq!(info.pairwise, vec![Cipher::Ccmp, Cipher::Tkip]);
        assert_eq!(info.akms, vec![Akm::Psk]);
        assert_eq!(info.caps, 0x0c);
        assert!(info.wpa2_personal());
        let own = supplicant_element();
        assert_eq!(
            own,
            [
                0x30, 0x14, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 2,
                0, 0
            ]
        );
        assert!(RsnInfo::parse(&own).unwrap().wpa2_personal());
    }

    #[test]
    fn defaults_and_refusals() {
        let short = RsnInfo::parse_body(&[1, 0]).unwrap();
        assert_eq!(short.akms, vec![Akm::Ieee8021x]);
        assert!(!short.wpa2_personal());
        for bad in [
            &[][..],
            &[2, 0],
            &[1, 0, 0, 0x0f],
            &[1, 0, 0, 0x0f, 0xac, 4, 0, 0],
            &[1, 0, 0, 0x0f, 0xac, 4, 17, 0],
            &[1, 0, 0, 0x0f, 0xac, 4, 2, 0, 0, 0x0f, 0xac, 4],
        ] {
            assert_eq!(
                RsnInfo::parse_body(bad),
                Err(WpaError::BadElement),
                "{bad:?}"
            );
        }
        assert_eq!(RsnInfo::parse(&[48, 5, 1, 0]), Err(WpaError::BadElement));
        let other = RsnInfo::parse_body(&[
            1, 0, 1, 2, 3, 4, 1, 0, 0, 0x0f, 0xac, 9, 1, 0, 0, 0x0f, 0xac, 6,
        ])
        .unwrap();
        assert_eq!(other.group, Cipher::Other(0xff));
        assert_eq!(other.pairwise, vec![Cipher::Other(9)]);
        assert_eq!(other.akms, vec![Akm::PskSha256]);
    }
}
