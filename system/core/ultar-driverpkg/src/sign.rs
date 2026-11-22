//! Canonical signed messages and the detached signature blob (`<key id>\n<base64 signature>`).

use crate::error::Coded;
use crate::keys::{sha256_hex, PrivateKey, TrustedKeys};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::fmt;

/// Domain tag of the message signed for a driver package.
pub const PACKAGE_DOMAIN: &str = "ultar-driver-v1";
/// Domain tag of the message signed for a depot index.
pub const INDEX_DOMAIN: &str = "ultar-driver-index-v1";

/// Why a signature was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// The signature blob is not `<key id>\n<base64 of 64 bytes>`.
    Malformed,
    /// The device has no trusted keys at all, so nothing can be accepted.
    NoTrustedKeys,
    /// The signature was made with a key that is not in the trusted set.
    UntrustedKey {
        /// Identifier of the foreign key as stated in the signature blob.
        key_id: String,
    },
    /// The key is trusted but the signature does not match the content (content was modified).
    Invalid,
}

impl Coded for SignatureError {
    fn code(&self) -> &'static str {
        match self {
            Self::Malformed => "driverpkg.signature.malformed",
            Self::NoTrustedKeys => "driverpkg.signature.no-trusted-keys",
            Self::UntrustedKey { .. } => "driverpkg.signature.untrusted-key",
            Self::Invalid => "driverpkg.signature.invalid",
        }
    }
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UntrustedKey { key_id } => write!(f, "{}: {key_id}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for SignatureError {}

/// The canonical message for a driver package: domain, manifest hash, payload hash.
pub fn package_message(manifest: &[u8], payload: &[u8]) -> String {
    format!(
        "{PACKAGE_DOMAIN}\n{}\n{}\n",
        sha256_hex(manifest),
        sha256_hex(payload)
    )
}

/// The canonical message for a depot index: domain and index hash.
pub fn index_message(index: &[u8]) -> String {
    format!("{INDEX_DOMAIN}\n{}\n", sha256_hex(index))
}

/// A detached signature together with the identifier of the key that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureBlob {
    /// Identifier of the signing key ([`crate::keys::PublicKey::key_id`]).
    pub key_id: String,
    /// The raw 64-byte Ed25519 signature.
    pub signature: [u8; 64],
}

impl SignatureBlob {
    /// Signs `message` with `key`.
    pub fn create(message: &str, key: &PrivateKey) -> Self {
        Self {
            key_id: key.public().key_id(),
            signature: key.sign(message.as_bytes()),
        }
    }

    /// Parses the textual form.
    pub fn parse(blob: &[u8]) -> Result<Self, SignatureError> {
        let text = std::str::from_utf8(blob).map_err(|_| SignatureError::Malformed)?;
        let mut lines = text.lines();
        let key_id = lines.next().unwrap_or_default().trim();
        let encoded = lines.next().unwrap_or_default().trim();
        let id_ok = key_id.len() == 16 && key_id.bytes().all(|b| b.is_ascii_hexdigit());
        if !id_ok || lines.any(|l| !l.trim().is_empty()) {
            return Err(SignatureError::Malformed);
        }
        let raw = B64.decode(encoded).map_err(|_| SignatureError::Malformed)?;
        let signature: [u8; 64] = raw
            .as_slice()
            .try_into()
            .map_err(|_| SignatureError::Malformed)?;
        Ok(Self {
            key_id: key_id.to_string(),
            signature,
        })
    }

    /// Renders the textual form stored in `sig.ed25519` / `index.toml.sig`.
    pub fn encode(&self) -> String {
        format!("{}\n{}\n", self.key_id, B64.encode(self.signature))
    }

    /// Checks the signature over `message` against the trusted keys.
    pub fn check(&self, message: &str, trusted: &TrustedKeys) -> Result<(), SignatureError> {
        if trusted.is_empty() {
            return Err(SignatureError::NoTrustedKeys);
        }
        let key = trusted
            .find(&self.key_id)
            .ok_or_else(|| SignatureError::UntrustedKey {
                key_id: self.key_id.clone(),
            })?;
        if key.verify(message.as_bytes(), &self.signature) {
            Ok(())
        } else {
            Err(SignatureError::Invalid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_messages() {
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            package_message(b"", b""),
            format!("ultar-driver-v1\n{empty}\n{empty}\n")
        );
        assert_eq!(
            index_message(b""),
            format!("ultar-driver-index-v1\n{empty}\n")
        );
        assert_ne!(package_message(b"a", b"b"), package_message(b"b", b"a"));
    }

    #[test]
    fn blob_round_trip_and_check() {
        let key = PrivateKey::generate();
        let trusted = TrustedKeys::from(key.public());
        let blob = SignatureBlob::create("hello\n", &key);
        let parsed = SignatureBlob::parse(blob.encode().as_bytes()).unwrap();
        assert_eq!(parsed, blob);
        assert_eq!(parsed.check("hello\n", &trusted), Ok(()));
        assert_eq!(
            parsed.check("hello!\n", &trusted),
            Err(SignatureError::Invalid)
        );
        assert_eq!(
            parsed.check("hello\n", &TrustedKeys::new()),
            Err(SignatureError::NoTrustedKeys)
        );
        let other = TrustedKeys::from(PrivateKey::generate().public());
        assert_eq!(
            parsed.check("hello\n", &other),
            Err(SignatureError::UntrustedKey {
                key_id: key.public().key_id()
            })
        );
    }

    #[test]
    fn malformed_blobs() {
        let good = SignatureBlob::create("m", &PrivateKey::generate()).encode();
        let (id, sig) = good.trim_end().split_once('\n').unwrap();
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"only-one-line".to_vec(),
            format!("{id}\n").into_bytes(),
            format!("short\n{sig}\n").into_bytes(),
            format!("{id}\nnot base64!\n").into_bytes(),
            format!("{id}\nQUJD\n").into_bytes(),
            format!("{id}\n{sig}\nextra\n").into_bytes(),
            vec![0xff, 0xfe, b'\n'],
        ];
        for case in cases {
            assert_eq!(SignatureBlob::parse(&case), Err(SignatureError::Malformed));
        }
        let err = SignatureError::UntrustedKey {
            key_id: "abc".into(),
        };
        assert_eq!(err.to_string(), "driverpkg.signature.untrusted-key: abc");
        assert_eq!(
            SignatureError::Invalid.to_string(),
            "driverpkg.signature.invalid"
        );
    }
}
