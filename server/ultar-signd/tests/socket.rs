//! End to end over a real Unix socket: service thread + client functions, and the results
//! checked the way the device checks them (ultar-pkg message layout, catalogue layout, SSHSIG).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, SigningKey, Verifier};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::Path;
use ultar_signd::audit::AuditLog;
use ultar_signd::keys::Role;
use ultar_signd::ratelimit::Limit;
use ultar_signd::server::{bind, serve_one, Signer};
use ultar_signd::{client, SignError};

/// Serves exactly `n` connections on a thread.
fn spawn(listener: UnixListener, mut signer: Signer, n: usize) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for stream in listener.incoming().take(n) {
            serve_one(&mut signer, stream.unwrap());
        }
    })
}

fn keys() -> [Option<SigningKey>; 3] {
    [1u8, 2, 3].map(|b| Some(SigningKey::from_bytes(&[b; 32])))
}

fn members(uap: &Path) -> Vec<(String, Vec<u8>)> {
    let mut ar = tar::Archive::new(std::fs::File::open(uap).unwrap());
    ar.entries()
        .unwrap()
        .map(|e| {
            let mut e = e.unwrap();
            let name = e.path().unwrap().display().to_string();
            let mut data = Vec::new();
            e.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}

/// Checks a `.uap` the way `ultar-pkg` does: message over the member digests.
fn uap_verifies(uap: &Path, key: &SigningKey) -> bool {
    let parts = members(uap);
    assert_eq!(parts.len(), 3);
    let msg = format!(
        "ultar-pkg-v1\n{}\n{}\n",
        hex::encode(Sha256::digest(&parts[0].1)),
        hex::encode(Sha256::digest(&parts[1].1))
    );
    let blob = String::from_utf8(parts[2].1.clone()).unwrap();
    let sig_bytes = B64.decode(blob.lines().nth(1).unwrap()).unwrap();
    let sig = Signature::from_bytes(&sig_bytes.try_into().unwrap());
    key.verifying_key().verify(msg.as_bytes(), &sig).is_ok()
}

#[test]
fn package_catalogue_and_release_signatures_verify_like_on_the_device() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sign.sock");
    let signer = Signer::with_keys(
        keys(),
        AuditLog::new(dir.path().join("audit.log")),
        Role::ALL.map(Limit::default_for),
        0,
    );
    let handle = spawn(bind(&sock).unwrap(), signer, 5);

    // .uap with the system key.
    let manifest = dir.path().join("m.toml");
    let payload = dir.path().join("p.tar.gz");
    std::fs::write(&manifest, "id = \"settings\"\n").unwrap();
    std::fs::write(&payload, b"\x1f\x8bpayload").unwrap();
    let out = dir.path().join("settings.uap");
    client::sign_uap(&sock, Role::System, &manifest, &payload, &out).unwrap();
    assert!(uap_verifies(&out, &SigningKey::from_bytes(&[2; 32])));
    assert!(!uap_verifies(&out, &SigningKey::from_bytes(&[3; 32])));
    let store = SigningKey::from_bytes(&[3; 32]).verifying_key();

    // Catalogue with the store key.
    let cat = dir.path().join("ultar-catalog.toml");
    std::fs::write(&cat, "format_version = 1\n").unwrap();
    let sig_file = client::sign_catalog(&sock, &cat).unwrap();
    let blob = std::fs::read_to_string(sig_file).unwrap();
    let sig_bytes = B64.decode(blob.lines().nth(1).unwrap()).unwrap();
    let sig = Signature::from_bytes(&sig_bytes.try_into().unwrap());
    let msg = format!(
        "ultar-catalog-v1\n{}\n",
        hex::encode(Sha256::digest(b"format_version = 1\n"))
    );
    assert!(store.verify(msg.as_bytes(), &sig).is_ok());

    // Release: SSHSIG armor.
    let img = dir.path().join("ultar.img");
    std::fs::write(&img, vec![7u8; 3 << 20]).unwrap();
    let sig_file = client::sign_release(&sock, &img).unwrap();
    assert!(std::fs::read_to_string(sig_file)
        .unwrap()
        .starts_with("-----BEGIN SSH SIGNATURE-----"));

    // Public key and a refused request.
    assert_eq!(
        client::public_key(&sock, Role::Store).unwrap(),
        hex::encode(store.as_bytes())
    );
    assert_eq!(
        client::request(&sock, "sign raw bytes"),
        Err(SignError::Refused("bad_request".into()))
    );
    handle.join().unwrap();
    let audit = std::fs::read_to_string(dir.path().join("audit.log")).unwrap();
    assert_eq!(audit.lines().count(), 4); // pubkey is not audited; the bad request is
}
