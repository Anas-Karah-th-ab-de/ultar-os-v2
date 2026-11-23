//! Device identity on the device side: the key-derived device id, the signed transcripts and the
//! server name they are bound to. Mirrors `ultraos-server/src/identity.rs`; the layout is fixed
//! by `docs/DEVICE-IDENTITY.md` §3–§4 and checked against the server in the end-to-end tests.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use sha2::{Digest, Sha256};

/// Length of an Ed25519 public key in bytes.
pub const PUBLIC_KEY_BYTES: usize = 32;
/// Length of an Ed25519 signature in bytes.
pub const SIGNATURE_BYTES: usize = 64;
/// Length of a server challenge in bytes.
pub const CHALLENGE_BYTES: usize = 32;

const ID_LABEL: &[u8] = b"ultar-device-v1";
const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// What a signature is for. Each purpose has its own transcript label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Registration of this device's key.
    Register,
    /// Request of a device token.
    Session,
    /// Authentication of the signalling socket.
    Signal,
    /// Upgrade of a registration from before device keys.
    Upgrade,
    /// Replacement, signed by the old device.
    ReplaceOld,
    /// Replacement, signed by the new device.
    ReplaceNew,
    /// Replacement of a lost device, signed by the new device.
    Recover,
}

impl Purpose {
    /// The domain-separation label that starts the transcript.
    pub fn label(self) -> &'static str {
        match self {
            Purpose::Register => "ultar-device-register-v1",
            Purpose::Session => "ultar-device-session-v1",
            Purpose::Signal => "ultar-device-signal-v1",
            Purpose::Upgrade => "ultar-device-upgrade-v1",
            Purpose::ReplaceOld => "ultar-device-replace-old-v1",
            Purpose::ReplaceNew => "ultar-device-replace-new-v1",
            Purpose::Recover => "ultar-device-recover-v1",
        }
    }
}

/// RFC 4648 base32 with the lower-case alphabet and without padding.
pub fn base32_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in bytes {
        acc = (acc << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(BASE32[((acc >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(BASE32[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

/// The device id of a public key: base32 of the first 160 bits of
/// `SHA-256("ultar-device-v1" ‖ public key)` (32 characters). No hardware property enters it.
pub fn device_id(public_key: &[u8; PUBLIC_KEY_BYTES]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ID_LABEL);
    hasher.update(public_key);
    base32_lower(&hasher.finalize()[..20])
}

/// Encodes bytes as base64url without padding (the wire form of keys, challenges, signatures).
pub fn encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

/// Decodes a base64url value of exactly `N` bytes.
pub fn decode_fixed<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() > N * 2 {
        return None;
    }
    <[u8; N]>::try_from(B64.decode(text).ok()?.as_slice()).ok()
}

/// The name of a server as it enters every transcript: the authority of its address in lower
/// case, without the default port. `None` for addresses that are not plain `http(s)://host[:port]`
/// (user info, odd characters) – nothing is signed for such an address.
pub fn server_name(base_url: &str) -> Option<String> {
    let lower = base_url.trim().to_ascii_lowercase();
    let (rest, default_port) = if let Some(rest) = lower.strip_prefix("https://") {
        (rest, ":443")
    } else {
        (lower.strip_prefix("http://")?, ":80")
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let name = authority.strip_suffix(default_port).unwrap_or(authority);
    let ok = (1..=255).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-:[]".contains(&b));
    ok.then(|| name.to_string())
}

/// The byte string a device signs.
#[derive(Debug, Clone, Copy)]
pub struct Transcript<'a> {
    /// What the signature is for.
    pub purpose: Purpose,
    /// Name of the server the signature is meant for.
    pub server: &'a str,
    /// Account the signature is meant for.
    pub account_id: i64,
    /// The server's challenge.
    pub challenge: &'a [u8; CHALLENGE_BYTES],
    /// Public key of the signer.
    pub subject_key: &'a [u8; PUBLIC_KEY_BYTES],
    /// Purpose-specific data (empty, a device id or the other party's key).
    pub extra: &'a [u8],
}

fn framed(out: &mut Vec<u8>, field: &[u8]) -> Option<()> {
    let len = u16::try_from(field.len()).ok()?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(field);
    Some(())
}

impl Transcript<'_> {
    /// `label ‖ 0 ‖ len16 ‖ server ‖ account(be64) ‖ challenge ‖ subject key ‖ len16 ‖ extra`.
    /// `None` when a variable field does not fit its 16-bit length (nothing is signed then).
    pub fn bytes(&self) -> Option<Vec<u8>> {
        let label = self.purpose.label().as_bytes();
        let mut out = Vec::with_capacity(label.len() + self.server.len() + self.extra.len() + 80);
        out.extend_from_slice(label);
        out.push(0);
        framed(&mut out, self.server.as_bytes())?;
        out.extend_from_slice(&self.account_id.to_be_bytes());
        out.extend_from_slice(self.challenge);
        out.extend_from_slice(self.subject_key);
        framed(&mut out, self.extra)?;
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_matches_rfc_4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "my"),
            ("fo", "mzxq"),
            ("foo", "mzxw6"),
            ("foob", "mzxw6yq"),
            ("fooba", "mzxw6ytb"),
            ("foobar", "mzxw6ytboi"),
        ] {
            assert_eq!(base32_lower(input.as_bytes()), expected);
        }
    }

    #[test]
    fn device_id_matches_the_vector_shared_with_the_server() {
        assert_eq!(device_id(&[0u8; 32]), "44ifisy5d2lrobapoboy7iir4a2ksvm7");
        assert_ne!(device_id(&[1u8; 32]), device_id(&[0u8; 32]));
        assert_eq!(device_id(&[9u8; 32]).len(), 32);
    }

    #[test]
    fn server_names_are_canonical() {
        for (url, name) in [
            ("https://ultar.code-craft.tech", "ultar.code-craft.tech"),
            ("https://Ultar.Code-Craft.tech/", "ultar.code-craft.tech"),
            (
                "https://ultar.code-craft.tech:443/v1",
                "ultar.code-craft.tech",
            ),
            (
                "https://ultar.code-craft.tech:8443",
                "ultar.code-craft.tech:8443",
            ),
            ("http://127.0.0.1:41234", "127.0.0.1:41234"),
            ("http://example.org:80?x", "example.org"),
            ("http://[::1]:8080/", "[::1]:8080"),
        ] {
            assert_eq!(server_name(url).as_deref(), Some(name), "{url}");
        }
        for bad in [
            "",
            "ultar.code-craft.tech",
            "ftp://x",
            "https://",
            "https://user@host",
            "https://real.example@evil.example/",
            "https://a b",
            "https://ü.example",
        ] {
            assert_eq!(server_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn transcript_layout_is_as_documented() {
        let (challenge, pk) = ([0xAA; 32], [0xBB; 32]);
        let t = Transcript {
            purpose: Purpose::Register,
            server: "ultar.example",
            account_id: 7,
            challenge: &challenge,
            subject_key: &pk,
            extra: b"xy",
        };
        let mut expected = b"ultar-device-register-v1\0".to_vec();
        expected.extend_from_slice(&[0, 13]);
        expected.extend_from_slice(b"ultar.example");
        expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 7]);
        expected.extend_from_slice(&[0xAA; 32]);
        expected.extend_from_slice(&[0xBB; 32]);
        expected.extend_from_slice(&[0, 2, b'x', b'y']);
        assert_eq!(t.bytes().unwrap(), expected);
        let long = vec![0u8; 70_000];
        assert!(Transcript { extra: &long, ..t }.bytes().is_none());
    }

    #[test]
    fn labels_differ_and_encoding_round_trips() {
        let all = [
            Purpose::Register,
            Purpose::Session,
            Purpose::Signal,
            Purpose::Upgrade,
            Purpose::ReplaceOld,
            Purpose::ReplaceNew,
            Purpose::Recover,
        ];
        let labels: std::collections::HashSet<_> = all.iter().map(|p| p.label()).collect();
        assert_eq!(labels.len(), all.len());
        let bytes = [5u8; 32];
        assert_eq!(decode_fixed::<32>(&encode(&bytes)), Some(bytes));
        assert_eq!(decode_fixed::<32>("AAAA"), None);
        assert_eq!(decode_fixed::<16>(&"A".repeat(400)), None);
        assert_eq!(decode_fixed::<16>("***"), None);
    }
}
