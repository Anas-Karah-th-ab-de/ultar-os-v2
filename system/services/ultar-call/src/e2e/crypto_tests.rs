use super::*;
use ed25519_dalek::{Signer, SigningKey};

fn device(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// Runs a whole handshake: both sides' keys and the transcript.
fn handshake() -> (CallKeys, CallKeys, Transcript, SigningKey, SigningKey) {
    let (a, b) = (device(1), device(2));
    let (ea, eb) = (Ephemeral::new(), Ephemeral::new());
    let t = Transcript {
        call: [7; CALL_ID],
        ea: ea.public,
        eb: eb.public,
        da: a.verifying_key().to_bytes(),
        db: b.verifying_key().to_bytes(),
    };
    let ka = keys(ea, &t.eb, &t, Role::Caller).unwrap();
    let kb = keys(eb, &t.ea, &t, Role::Callee).unwrap();
    (ka, kb, t, a, b)
}

#[test]
fn both_sides_get_matching_keys_one_per_direction() {
    let (mut ka, mut kb, t, _, _) = handshake();
    assert_eq!(ka.transcript, kb.transcript);
    assert_eq!(ka.transcript, t.hash());
    let m = ka.signal_out.seal(b"x", b"hello");
    assert_eq!(kb.signal_in.open(b"x", &m).unwrap(), b"hello");
    let v = kb.media.as_mut().unwrap().out.seal(b"", b"voice");
    assert_eq!(
        ka.media.as_mut().unwrap().inn.open(b"", &v).unwrap(),
        b"voice"
    );
    // a key of one direction does not open the other
    let back = kb.signal_out.seal(b"x", b"r");
    assert!(kb.signal_in.open(b"x", &back).is_err());
    // signalling and media keys differ
    let s = ka.signal_out.seal(b"", b"s");
    assert!(kb.media.as_mut().unwrap().inn.open(b"", &s).is_err());
}

#[test]
fn a_message_is_taken_once_and_unchanged_only() {
    let (ka, kb, _, _, _) = handshake();
    let (mut out, mut inn) = (ka.media.unwrap().out, kb.media.unwrap().inn);
    let m = out.seal(b"aad", b"frame");
    assert!(inn.open(b"other", &m).is_err(), "aad is bound");
    let mut changed = m.clone();
    changed[10] ^= 1;
    assert!(inn.open(b"aad", &changed).is_err());
    assert!(inn.open(b"aad", &m).is_ok());
    assert_eq!(inn.open(b"aad", &m), Err(CryptoError::Open), "replay");
    assert!(inn.open(b"aad", &m[..5]).is_err());
}

#[test]
fn signatures_bind_the_whole_transcript() {
    let (_, _, t, a, b) = handshake();
    let sig = b.sign(&t.to_sign()).to_bytes();
    assert_eq!(verify(&t, &t.db, &sig), Ok(()));
    assert_eq!(verify(&t, &t.da, &sig), Err(CryptoError::BadSignature));
    let mut other = t;
    other.ea[0] ^= 1;
    assert_eq!(verify(&other, &t.db, &sig), Err(CryptoError::BadSignature));
    // a signature without the label (as another protocol would make it) is refused
    let bare = b.sign(&t.hash()).to_bytes();
    assert_eq!(verify(&t, &t.db, &bare), Err(CryptoError::BadSignature));
    assert_eq!(verify(&t, &t.db, &[0; 3]), Err(CryptoError::Malformed));
    let _ = a;
}

#[test]
fn a_server_that_swaps_an_ephemeral_key_gets_different_keys() {
    let (ea, eb, mitm) = (Ephemeral::new(), Ephemeral::new(), Ephemeral::new());
    let t = Transcript {
        call: [1; CALL_ID],
        ea: ea.public,
        eb: eb.public,
        da: [3; 32],
        db: [4; 32],
    };
    let mut ka = keys(ea, &t.eb, &t, Role::Caller).unwrap();
    // the callee saw the server's key instead of the caller's
    let seen = Transcript {
        ea: mitm.public,
        ..t
    };
    let mut kb = keys(eb, &seen.ea, &seen, Role::Callee).unwrap();
    assert_ne!(ka.transcript, kb.transcript);
    let m = ka.signal_out.seal(b"", b"x");
    assert!(kb.signal_in.open(b"", &m).is_err());
    assert!(
        keys(Ephemeral::new(), &[0; 32], &t, Role::Caller).is_err(),
        "low-order key"
    );
}

#[test]
fn the_safety_number_is_the_same_on_both_sides() {
    let (a, b) = ([1u8; 32], [2u8; 32]);
    let n = safety_number(&a, &b);
    assert_eq!(n, safety_number(&b, &a));
    assert_eq!(n.split(' ').count(), 12);
    assert!(n
        .split(' ')
        .all(|g| g.len() == 5 && g.bytes().all(|c| c.is_ascii_digit())));
    assert_ne!(n, safety_number(&a, &[3; 32]));
}
