//! The public SSH key the installer can take for the developer access (`ultar-sshd`, port 2222):
//! one OpenSSH line `<type> <base64> [comment]`. Only Ed25519 and ECDSA P-256 – their lines fit
//! one value of the answers record ([`crate::engine::answers::MAX_VALUE`]); RSA keys are longer
//! and go in later through the web administration or `ultar-sshd schluessel`.

use crate::engine::answers::MAX_VALUE;

/// Key types accepted here.
pub const TYPES: [&str; 2] = ["ssh-ed25519", "ecdsa-sha2-nistp256"];

/// `true` for an empty field (the key is optional) or one valid public key line.
pub fn valid_or_empty(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || valid(line)
}

/// `true` for one public key line of an accepted type whose key blob names the same type.
pub fn valid(line: &str) -> bool {
    let line = line.trim();
    if line.len() > MAX_VALUE || line.chars().any(char::is_control) {
        return false;
    }
    let mut parts = line.split(' ').filter(|p| !p.is_empty());
    let (Some(kind), Some(blob)) = (parts.next(), parts.next()) else {
        return false;
    };
    if !TYPES.contains(&kind) {
        return false;
    }
    let Some(raw) = base64(blob) else {
        return false;
    };
    // the blob starts with the type as an SSH string: u32 length (big endian), then the name
    let n = kind.len();
    raw.len() > 4 + n && raw[..4] == (n as u32).to_be_bytes() && &raw[4..4 + n] == kind.as_bytes()
}

/// Standard base64 with padding; `None` for any other character or a broken length.
fn base64(s: &str) -> Option<Vec<u8>> {
    if s.is_empty() || !s.len().is_multiple_of(4) {
        return None;
    }
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes = s.as_bytes();
    let pad = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let last = i == bytes.len() / 4 - 1;
        let mut acc = 0u32;
        for (j, &c) in chunk.iter().enumerate() {
            let v = if c == b'=' && last && j >= 4 - pad {
                0
            } else {
                val(c)?
            };
            acc = acc << 6 | v;
        }
        let take = if last { 3 - pad } else { 3 };
        out.extend_from_slice(&acc.to_be_bytes()[1..1 + take]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl anas@pc";

    #[test]
    fn ed25519_line_is_valid() {
        assert!(valid(ED));
        assert!(valid_or_empty(ED));
        assert!(valid(&format!("  {ED}  ")));
    }

    #[test]
    fn empty_is_allowed_only_as_optional() {
        assert!(valid_or_empty(""));
        assert!(valid_or_empty("   "));
        assert!(!valid(""));
    }

    #[test]
    fn wrong_lines_are_refused() {
        // type does not match the blob
        assert!(!valid("ecdsa-sha2-nistp256 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"));
        // RSA is not taken here, a private key never
        assert!(!valid("ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQ== x"));
        assert!(!valid("-----BEGIN OPENSSH PRIVATE KEY-----"));
        // broken base64, missing blob, control characters, too long
        assert!(!valid("ssh-ed25519 AAAAC3Nza!1lZDI1NTE5"));
        assert!(!valid("ssh-ed25519"));
        assert!(!valid(&format!("{ED}\nssh-ed25519 AAAA")));
        assert!(!valid(&format!("{ED} {}", "x".repeat(300))));
    }

    #[test]
    fn base64_round_numbers() {
        assert_eq!(base64("TWFu").as_deref(), Some(&b"Man"[..]));
        assert_eq!(base64("TWE=").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(base64("TQ==").as_deref(), Some(&b"M"[..]));
        assert_eq!(base64("TQ="), None);
        assert_eq!(base64("T==="), None);
    }
}
