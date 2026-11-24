//! Keys of WPA2-Personal (IEEE 802.11-2016 12.7.1): the PMK from the
//! passphrase (PBKDF2-HMAC-SHA1, 4096 rounds, J.4), the PRF (12.7.1.2) and
//! the pairwise transient key for CCMP (KCK, KEK, TK). Every key is wiped
//! when dropped and never printed.

use super::WpaError;
use crate::Mac;
use hmac::{Hmac, Mac as _};
use sha1::Sha1;
use zeroize::Zeroize;

/// Rounds of PBKDF2 for the PMK.
pub const PBKDF2_ROUNDS: u32 = 4096;

/// The pairwise master key (32 bytes).
#[derive(Clone, PartialEq, Eq)]
pub struct Pmk([u8; 32]);

impl Drop for Pmk {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Pmk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pmk(..)")
    }
}

impl Pmk {
    /// The PMK of a passphrase (8–63 printable ASCII characters) for an
    /// SSID, or a PSK given as 64 hexadecimal digits.
    pub fn from_passphrase(passphrase: &str, ssid: &[u8]) -> Result<Pmk, WpaError> {
        let bytes = passphrase.as_bytes();
        if bytes.len() == 64 && bytes.iter().all(u8::is_ascii_hexdigit) {
            let mut key = [0u8; 32];
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                key[i] = (hex(pair[0]) << 4) | hex(pair[1]);
            }
            return Ok(Pmk(key));
        }
        if !(8..=63).contains(&bytes.len()) || !bytes.iter().all(|b| (0x20..=0x7e).contains(b)) {
            return Err(WpaError::BadPassphrase);
        }
        if ssid.is_empty() || ssid.len() > crate::ie::MAX_SSID {
            return Err(WpaError::BadSsid);
        }
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha1>(bytes, ssid, PBKDF2_ROUNDS, &mut key);
        Ok(Pmk(key))
    }

    /// A PMK from its 32 bytes (a saved network).
    pub fn from_bytes(bytes: [u8; 32]) -> Pmk {
        Pmk(bytes)
    }

    /// The key bytes (for storing it 0600; whoever calls this is responsible
    /// for where they go).
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

fn hex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// HMAC-SHA1 of `parts` (concatenated) with `key`.
pub fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    // HMAC accepts keys of any length: the error case cannot happen.
    let mut mac = match <Hmac<Sha1> as hmac::Mac>::new_from_slice(key) {
        Ok(m) => m,
        Err(_) => return [0; 20],
    };
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// The PRF of 12.7.1.2 with HMAC-SHA1: `out.len()` bytes.
pub fn prf(key: &[u8], label: &[u8], data: &[u8], out: &mut [u8]) {
    for (i, chunk) in out.chunks_mut(20).enumerate() {
        let mut block = hmac_sha1(key, &[label, &[0], data, &[i as u8]]);
        chunk.copy_from_slice(&block[..chunk.len()]);
        block.zeroize();
    }
}

/// The pairwise transient key for CCMP-128: KCK ‖ KEK ‖ TK.
#[derive(Clone, PartialEq, Eq)]
pub struct Ptk([u8; 48]);

impl Drop for Ptk {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Ptk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ptk(..)")
    }
}

impl Ptk {
    /// Derives the PTK from the PMK, both addresses and both nonces.
    pub fn derive(pmk: &Pmk, aa: &Mac, spa: &Mac, anonce: &[u8; 32], snonce: &[u8; 32]) -> Ptk {
        let mut data = [0u8; 76];
        let (lo, hi) = if aa < spa { (aa, spa) } else { (spa, aa) };
        data[..6].copy_from_slice(lo);
        data[6..12].copy_from_slice(hi);
        let (lo, hi) = if anonce < snonce {
            (anonce, snonce)
        } else {
            (snonce, anonce)
        };
        data[12..44].copy_from_slice(lo);
        data[44..].copy_from_slice(hi);
        let mut out = [0u8; 48];
        prf(&pmk.0, b"Pairwise key expansion", &data, &mut out);
        Ptk(out)
    }

    /// Key confirmation key (MIC of EAPOL-Key frames).
    pub fn kck(&self) -> &[u8] {
        &self.0[..16]
    }

    /// Key encryption key (key data of EAPOL-Key frames).
    pub fn kek(&self) -> &[u8] {
        &self.0[16..32]
    }

    /// Temporal key for CCMP, installed in the driver.
    pub fn tk(&self) -> &[u8] {
        &self.0[32..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn psk_test_vectors_of_the_standard() {
        // IEEE 802.11-2016 J.4.2.
        let pmk = Pmk::from_passphrase("password", b"IEEE").unwrap();
        assert_eq!(
            pmk.expose().to_vec(),
            unhex("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e")
        );
        let pmk = Pmk::from_passphrase("ThisIsAPassword", b"ThisIsASSID").unwrap();
        assert_eq!(
            pmk.expose().to_vec(),
            unhex("0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af")
        );
        assert!(!format!("{pmk:?}").contains("0d"));
    }

    #[test]
    fn passphrase_rules() {
        let hex64 = "0DC0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af";
        let pmk = Pmk::from_passphrase(hex64, b"").unwrap();
        assert_eq!(pmk.expose()[0], 0x0d);
        assert_eq!(pmk, Pmk::from_bytes(*pmk.expose()));
        for bad in [
            "short",
            "tab\tinside1",
            &"x".repeat(64),
            &"y".repeat(65),
            "ünicode-pass",
        ] {
            assert_eq!(
                Pmk::from_passphrase(bad, b"s"),
                Err(WpaError::BadPassphrase),
                "{bad}"
            );
        }
        assert_eq!(
            Pmk::from_passphrase("goodpassword", b""),
            Err(WpaError::BadSsid)
        );
        assert_eq!(
            Pmk::from_passphrase("goodpassword", &[b'a'; 33]),
            Err(WpaError::BadSsid)
        );
    }

    #[test]
    fn prf_matches_hmac_sha1_reference() {
        // Reference: Python hmac (HMAC-SHA1) of "prefix" 0x00 "Hi There" i,
        // key 0x0b * 20 (IEEE 802.11 PRF test vector, J.3.2 case 1).
        let mut out = [0u8; 64];
        prf(&[0x0b; 20], b"prefix", b"Hi There", &mut out);
        assert_eq!(
            out.to_vec(),
            unhex(
                "bcd4c650b30b9684951829e0d75f9d54b862175ed9f00606e17d8da35402ffee\
                 75df78c3d31e0f889f012120c0862beb67753e7439ae242edb8373698356cf5a"
            )
        );
    }
}
