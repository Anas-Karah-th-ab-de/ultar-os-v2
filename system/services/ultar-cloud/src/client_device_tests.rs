use super::*;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

struct MemoryKey(SigningKey);

impl KeyStore for MemoryKey {
    fn public_key(&self) -> Result<[u8; 32], crate::keystore::KeyStoreError> {
        Ok(self.0.verifying_key().to_bytes())
    }
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], crate::keystore::KeyStoreError> {
        Ok(self.0.sign(message).to_bytes())
    }
}

fn key(seed: u8) -> MemoryKey {
    MemoryKey(SigningKey::from_bytes(&[seed; 32]))
}

fn challenge(server: &str, account_id: i64) -> Challenge {
    Challenge {
        id: "id".into(),
        bytes: [9; 32],
        account_id,
        server: server.into(),
    }
}

fn verifies(
    by: &MemoryKey,
    purpose: Purpose,
    c: &Challenge,
    extra: &[u8],
    signature: &str,
) -> bool {
    let public = by.public_key().unwrap();
    let bytes = Transcript {
        purpose,
        server: &c.server,
        account_id: c.account_id,
        challenge: &c.bytes,
        subject_key: &public,
        extra,
    }
    .bytes()
    .unwrap();
    let Some(raw) = identity::decode_fixed::<64>(signature) else {
        return false;
    };
    VerifyingKey::from_bytes(&public)
        .unwrap()
        .verify_strict(&bytes, &Signature::from_bytes(&raw))
        .is_ok()
}

#[test]
fn the_old_device_signs_a_replacement_only_for_its_own_server_and_account() {
    let (old, new) = (key(1), key(2));
    let request = ReplaceRequest {
        challenge: challenge("ultar.example", 7),
        new_key: new.public_key().unwrap(),
    };
    let grant = authorize_replacement(&old, "ultar.example", 7, &request).unwrap();
    assert_eq!(grant.old_key, old.public_key().unwrap());
    assert_eq!(grant.old_device_id, identity::device_id(&grant.old_key));
    assert!(verifies(
        &old,
        Purpose::ReplaceOld,
        &request.challenge,
        &request.new_key,
        &grant.signature
    ));
    // The consent names the new key: it is worthless for any other key.
    assert!(!verifies(
        &old,
        Purpose::ReplaceOld,
        &request.challenge,
        &[3; 32],
        &grant.signature
    ));
    for (server, account) in [("evil.example", 7), ("ultar.example", 8)] {
        assert!(matches!(
            authorize_replacement(&old, server, account, &request),
            Err(CloudError::ServerIdentityMismatch)
        ));
    }
}

#[test]
fn signalling_frames_are_built_and_foreign_challenges_refused() {
    let device = key(1);
    let hello: Value = serde_json::from_str(&signal_hello(&device).unwrap()).unwrap();
    assert_eq!(hello["type"], "hello");
    assert_eq!(
        hello["device_id"],
        identity::device_id(&device.public_key().unwrap())
    );

    let c = challenge("ultar.example", 7);
    let frame = json!({"type":"challenge","challenge": identity::encode(&c.bytes),
                       "server": "ultar.example", "account_id": 7})
    .to_string();
    let auth: Value =
        serde_json::from_str(&signal_auth(&device, "ultar.example", 7, &frame).unwrap()).unwrap();
    assert_eq!(auth["type"], "auth");
    assert!(verifies(
        &device,
        Purpose::Signal,
        &c,
        b"",
        auth["signature"].as_str().unwrap()
    ));
    // a server that relays the challenge of another server, or names another account
    assert!(matches!(
        signal_auth(&device, "other.example", 7, &frame),
        Err(CloudError::ServerIdentityMismatch)
    ));
    assert!(matches!(
        signal_auth(&device, "ultar.example", 8, &frame),
        Err(CloudError::ServerIdentityMismatch)
    ));
    for bad in [
        "",
        "not json",
        r#"{"type":"ready"}"#,
        r#"{"type":"challenge","challenge":"AAAA","server":"ultar.example","account_id":7}"#,
        r#"{"type":"challenge","challenge":5,"server":"ultar.example","account_id":7}"#,
        r#"{"type":"challenge","server":"ultar.example","account_id":7}"#,
    ] {
        assert!(matches!(
            signal_auth(&device, "ultar.example", 7, bad),
            Err(CloudError::InvalidResponse)
        ));
    }
}

#[test]
fn device_calls_need_an_account_before_anything_is_sent() {
    let client = Client::new("http://127.0.0.1:9", None);
    let device = key(1);
    assert!(matches!(
        client.register_device(&device, ""),
        Err(CloudError::NotSignedIn)
    ));
    assert!(matches!(client.devices(), Err(CloudError::NotSignedIn)));
    assert!(matches!(
        client.replace_begin(&device),
        Err(CloudError::NotSignedIn)
    ));
    assert!(matches!(
        client.recover_start(&device, "p", "d"),
        Err(CloudError::NotSignedIn)
    ));
    assert!(matches!(
        client.recover_finish(&device, "d", ""),
        Err(CloudError::NotSignedIn)
    ));
    assert!(matches!(
        client.upgrade_legacy_device(&device, "x", ""),
        Err(CloudError::NotSignedIn)
    ));
    assert!(matches!(
        client.change_password("a", "b"),
        Err(CloudError::NotSignedIn)
    ));
    assert_eq!(client.server_name().unwrap(), "127.0.0.1:9");
    let odd = Client::new("https://user@host", Some("t".into()));
    assert!(matches!(
        odd.server_name(),
        Err(CloudError::ServerIdentityMismatch)
    ));
    assert!(matches!(
        odd.register_device(&device, ""),
        Err(CloudError::ServerIdentityMismatch)
    ));
}
