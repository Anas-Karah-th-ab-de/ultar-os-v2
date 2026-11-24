//! The wire protocol: one request line in, one answer line out.
//!
//! ```text
//! pkg-system <hex sha256 manifest> <hex sha256 payload>   → system key, "ultar-pkg-v1\n<m>\n<p>\n"
//! pkg-store  <hex sha256 manifest> <hex sha256 payload>   → store key,  "ultar-pkg-v1\n<m>\n<p>\n"
//! catalog    <hex sha256 body>                            → store key,  "ultar-catalog-v1\n<b>\n"
//! release    <hex sha512 file>                            → os key,     SSHSIG namespace "ultar-release"
//! pubkey     <os|system|store>                            → the public key (no signature)
//! answer:    ok <key-id> <base64 signature> | ok <hex public key> | err <code>
//! ```
//!
//! The service builds every signed message itself from fixed-length lowercase hex digests, so the
//! caller cannot choose the signed bytes beyond the digest, and the prefixes keep the domains
//! apart (the same layouts as `ultar-pkg` and `ultar-store`, verified there).

use crate::error::SignError;
use crate::keys::Role;
use crate::sshsig;

/// Longest request line accepted (bytes, including the newline).
pub const MAX_LINE: usize = 512;
/// SSHSIG namespace of release files.
pub const RELEASE_NAMESPACE: &str = "ultar-release";

/// One parsed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Sign a `.uap` package digest pair with the system or the store key.
    Package {
        /// [`Role::System`] or [`Role::Store`].
        role: Role,
        /// Hex SHA-256 of the manifest bytes.
        manifest: String,
        /// Hex SHA-256 of the payload bytes.
        payload: String,
    },
    /// Sign a catalogue body digest with the store key.
    Catalog {
        /// Hex SHA-256 of the body.
        body: String,
    },
    /// Sign a release file digest (SSHSIG) with the OS key.
    Release {
        /// Hex SHA-512 of the file.
        sha512: String,
    },
    /// Tell the public key of a role.
    PublicKey(Role),
}

fn hex_digest(word: Option<&str>, len: usize) -> Result<String, SignError> {
    let w = word.ok_or(SignError::BadRequest)?;
    if w.len() != len
        || !w
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(SignError::BadRequest);
    }
    Ok(w.to_string())
}

impl Request {
    /// Parses one line (without or with its trailing newline). Extra words are refused.
    pub fn parse(line: &str) -> Result<Request, SignError> {
        if line.len() > MAX_LINE {
            return Err(SignError::BadRequest);
        }
        let mut words = line.trim_end_matches('\n').split(' ');
        let op = words.next().unwrap_or("");
        let req = match op {
            "pkg-system" | "pkg-store" => Request::Package {
                role: if op == "pkg-system" {
                    Role::System
                } else {
                    Role::Store
                },
                manifest: hex_digest(words.next(), 64)?,
                payload: hex_digest(words.next(), 64)?,
            },
            "catalog" => Request::Catalog {
                body: hex_digest(words.next(), 64)?,
            },
            "release" => Request::Release {
                sha512: hex_digest(words.next(), 128)?,
            },
            "pubkey" => Request::PublicKey(
                words
                    .next()
                    .and_then(Role::parse)
                    .ok_or(SignError::BadRequest)?,
            ),
            _ => return Err(SignError::BadRequest),
        };
        if words.next().is_some() {
            return Err(SignError::BadRequest);
        }
        Ok(req)
    }

    /// The key this request uses.
    pub fn role(&self) -> Role {
        match self {
            Self::Package { role, .. } => *role,
            Self::Catalog { .. } => Role::Store,
            Self::Release { .. } => Role::Os,
            Self::PublicKey(role) => *role,
        }
    }

    /// Short operation name for the audit log.
    pub fn op(&self) -> &'static str {
        match self {
            Self::Package {
                role: Role::System, ..
            } => "pkg-system",
            Self::Package { .. } => "pkg-store",
            Self::Catalog { .. } => "catalog",
            Self::Release { .. } => "release",
            Self::PublicKey(_) => "pubkey",
        }
    }

    /// The digest(s) for the audit log.
    pub fn digest(&self) -> String {
        match self {
            Self::Package {
                manifest, payload, ..
            } => format!("{manifest}:{payload}"),
            Self::Catalog { body } => body.clone(),
            Self::Release { sha512 } => sha512.clone(),
            Self::PublicKey(_) => "-".into(),
        }
    }

    /// The exact bytes that are signed (`None` for [`Request::PublicKey`]).
    pub fn message(&self) -> Option<Vec<u8>> {
        match self {
            Self::Package {
                manifest, payload, ..
            } => Some(format!("ultar-pkg-v1\n{manifest}\n{payload}\n").into_bytes()),
            Self::Catalog { body } => Some(format!("ultar-catalog-v1\n{body}\n").into_bytes()),
            Self::Release { sha512 } => {
                let raw = hex::decode(sha512).ok()?;
                Some(sshsig::signed_data(RELEASE_NAMESPACE, &raw))
            }
            Self::PublicKey(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H64: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn parses_every_operation() {
        let r = Request::parse(&format!("pkg-store {H64} {H64}\n")).unwrap();
        assert_eq!(r.role(), Role::Store);
        assert_eq!(r.op(), "pkg-store");
        assert_eq!(
            r.message().unwrap(),
            format!("ultar-pkg-v1\n{H64}\n{H64}\n").into_bytes()
        );
        let r = Request::parse(&format!("pkg-system {H64} {H64}")).unwrap();
        assert_eq!((r.role(), r.op()), (Role::System, "pkg-system"));
        let r = Request::parse(&format!("catalog {H64}")).unwrap();
        assert_eq!(r.role(), Role::Store);
        assert_eq!(
            r.message().unwrap(),
            format!("ultar-catalog-v1\n{H64}\n").into_bytes()
        );
        let r = Request::parse(&format!("release {H64}{H64}")).unwrap();
        assert_eq!(r.role(), Role::Os);
        assert!(r.message().unwrap().starts_with(b"SSHSIG"));
        let r = Request::parse("pubkey os").unwrap();
        assert_eq!(r, Request::PublicKey(Role::Os));
        assert_eq!(r.message(), None);
        assert_eq!(r.digest(), "-");
    }

    #[test]
    fn refuses_malformed_lines() {
        let upper = H64.to_uppercase();
        for bad in [
            String::new(),
            "sign anything".into(),
            format!("catalog {upper}"),
            format!("catalog {}", &H64[1..]),
            format!("catalog {H64} extra"),
            format!("catalog  {H64}"),
            format!("pkg-store {H64}"),
            format!("release {H64}"),
            "pubkey root".into(),
            "pubkey".into(),
            format!("catalog {}", "a".repeat(600)),
            format!("catalog {}", "g".repeat(64)),
        ] {
            assert_eq!(Request::parse(&bad), Err(SignError::BadRequest), "{bad}");
        }
    }

    #[test]
    fn fuzz_never_panics_and_only_accepts_canonical_lines() {
        // Fixed-seed generator (xorshift) – deterministic.
        let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
        let alphabet = b"pkg-storecatalogrelease pubkey0123456789abcdefABCDEF \n\t\0";
        for _ in 0..20_000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let len = (s % 200) as usize;
            let line: String = (0..len)
                .map(|i| {
                    let x = s.rotate_left(i as u32 % 64);
                    alphabet[(x % alphabet.len() as u64) as usize] as char
                })
                .collect();
            if let Ok(req) = Request::parse(&line) {
                // Whatever parses re-serialises to a signable message or is a pubkey request.
                assert!(req.message().is_some() || matches!(req, Request::PublicKey(_)));
            }
        }
    }
}
