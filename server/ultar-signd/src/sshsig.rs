//! OpenSSH signature format (`SSHSIG`, PROTOCOL.sshsig) for release files.
//!
//! Chosen because every current Linux, macOS and Windows 10/11 machine has `ssh-keygen`, so a
//! user can check a download without installing anything:
//!
//! ```text
//! ssh-keygen -Y verify -f ultar-allowed-signers -I release@ultar.code-craft.tech \
//!            -n ultar-release -s ultar-os.img.sig < ultar-os.img
//! ```
//!
//! Signed data = `"SSHSIG" ‖ string(namespace) ‖ string("") ‖ string("sha512") ‖ string(H(file))`.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};

const MAGIC: &[u8] = b"SSHSIG";
const HASH: &str = "sha512";
const KEY_TYPE: &str = "ssh-ed25519";
/// Principal written into the published `allowed_signers` file.
pub const PRINCIPAL: &str = "release@ultar.code-craft.tech";

fn put_string(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
}

/// The bytes that are signed for a file whose SHA-512 is `sha512` (64 raw bytes).
pub fn signed_data(namespace: &str, sha512: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    put_string(&mut out, namespace.as_bytes());
    put_string(&mut out, b"");
    put_string(&mut out, HASH.as_bytes());
    put_string(&mut out, sha512);
    out
}

/// The SSH wire encoding of an Ed25519 public key.
pub fn public_blob(key: &VerifyingKey) -> Vec<u8> {
    let mut out = Vec::new();
    put_string(&mut out, KEY_TYPE.as_bytes());
    put_string(&mut out, key.as_bytes());
    out
}

/// One `authorized_keys`-style line: `ssh-ed25519 AAAA…`.
pub fn public_line(key: &VerifyingKey) -> String {
    format!("{KEY_TYPE} {}", B64.encode(public_blob(key)))
}

/// One line of an `allowed_signers` file restricted to `namespace`.
pub fn allowed_signers_line(key: &VerifyingKey, namespace: &str) -> String {
    format!(
        "{PRINCIPAL} namespaces=\"{namespace}\" {}",
        public_line(key)
    )
}

/// The armored signature file for `signature` over [`signed_data`].
pub fn armor(key: &VerifyingKey, namespace: &str, signature: &Signature) -> String {
    let mut blob = MAGIC.to_vec();
    blob.extend_from_slice(&1u32.to_be_bytes());
    put_string(&mut blob, &public_blob(key));
    put_string(&mut blob, namespace.as_bytes());
    put_string(&mut blob, b"");
    put_string(&mut blob, HASH.as_bytes());
    let mut sig = Vec::new();
    put_string(&mut sig, KEY_TYPE.as_bytes());
    put_string(&mut sig, &signature.to_bytes());
    put_string(&mut blob, &sig);
    let text = B64.encode(blob);
    let mut out = String::from("-----BEGIN SSH SIGNATURE-----\n");
    for chunk in text.as_bytes().chunks(70) {
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push('\n');
    }
    out.push_str("-----END SSH SIGNATURE-----\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha512};

    #[test]
    fn armored_signature_verifies_with_ssh_keygen_when_available() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let data = b"ultar release image bytes";
        let digest = Sha512::digest(data);
        let sig = key.sign(&signed_data("ultar-release", &digest));
        let armored = armor(&key.verifying_key(), "ultar-release", &sig);
        assert!(armored.starts_with("-----BEGIN SSH SIGNATURE-----\n"));
        assert!(armored
            .lines()
            .all(|l| l.len() <= 70 || l.starts_with("-----")));
        let Ok(out) = std::process::Command::new("ssh-keygen").arg("-?").output() else {
            return; // no OpenSSH on this machine: format checked above only
        };
        let _ = out;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.img");
        std::fs::write(&file, data).unwrap();
        std::fs::write(dir.path().join("f.img.sig"), &armored).unwrap();
        std::fs::write(
            dir.path().join("signers"),
            allowed_signers_line(&key.verifying_key(), "ultar-release") + "\n",
        )
        .unwrap();
        let verify = |ns: &str| {
            std::process::Command::new("ssh-keygen")
                .args(["-Y", "verify", "-f"])
                .arg(dir.path().join("signers"))
                .args(["-I", PRINCIPAL, "-n", ns, "-s"])
                .arg(dir.path().join("f.img.sig"))
                .stdin(std::fs::File::open(&file).unwrap())
                .output()
                .unwrap()
                .status
                .success()
        };
        assert!(verify("ultar-release"));
        assert!(!verify("file"));
        std::fs::write(&file, b"tampered").unwrap();
        assert!(!verify("ultar-release"));
    }

    #[test]
    fn public_line_has_the_ssh_prefix() {
        let key = SigningKey::from_bytes(&[1u8; 32]).verifying_key();
        assert!(public_line(&key).starts_with("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5"));
        assert!(allowed_signers_line(&key, "ultar-release")
            .starts_with("release@ultar.code-craft.tech namespaces=\"ultar-release\" "));
    }
}
