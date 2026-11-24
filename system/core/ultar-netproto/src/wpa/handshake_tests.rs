use super::*;
use crate::wpa::rsn::supplicant_element;

/// The recorded handshake of `tests/vectors/handshake.txt` (made by an
/// independent reference: Python hashlib/hmac and `cryptography`'s AES key
/// wrap, see `gen_handshake.py`).
fn vector(name: &str) -> Vec<u8> {
    let text = include_str!("../../tests/vectors/handshake.txt");
    let line = text
        .lines()
        .find(|l| l.split(' ').next() == Some(name))
        .unwrap();
    let hex = line.split(' ').nth(1).unwrap();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn mac(name: &str) -> Mac {
    vector(name).try_into().unwrap()
}

fn supplicant() -> Supplicant {
    let pmk = Pmk::from_bytes(vector("pmk").try_into().unwrap());
    Supplicant::new(
        pmk,
        mac("aa"),
        mac("spa"),
        vector("rsn_ap"),
        supplicant_element(),
        vector("snonce").try_into().unwrap(),
    )
}

#[test]
fn the_recorded_handshake_gives_the_same_answers_and_keys() {
    let pass = String::from_utf8(vector("passphrase")).unwrap();
    let pmk = Pmk::from_passphrase(&pass, &vector("ssid")).unwrap();
    assert_eq!(pmk.expose().to_vec(), vector("pmk"));
    assert_eq!(supplicant_element(), vector("rsn_sta"));
    let mut s = supplicant();
    assert_eq!(s.receive(&vector("m1")), Ok(Step::Send(vector("m2"))));
    match s.receive(&vector("m3")).unwrap() {
        Step::Complete { reply, tk, group } => {
            assert_eq!(reply, vector("m4"));
            assert_eq!(tk.to_vec(), vector("tk"));
            assert_eq!(group.index, 1);
            assert_eq!(group.key.to_vec(), vector("gtk"));
            assert_eq!(group.rsc, [5, 0, 0, 0, 0, 0, 0, 0]);
            assert!(!format!("{group:?}").contains("0011"));
        }
        other => panic!("{other:?}"),
    }
    assert!(s.complete());
    match s.receive(&vector("g1")).unwrap() {
        Step::Rekey { reply, group } => {
            assert_eq!(reply, vector("g2"));
            assert_eq!((group.index, group.key.to_vec()), (2, vector("gtk2")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn keys_are_never_installed_twice() {
    let mut s = supplicant();
    s.receive(&vector("m1")).unwrap();
    assert!(matches!(
        s.receive(&vector("m3")),
        Ok(Step::Complete { .. })
    ));
    // The same message 3 again: a replay.
    assert_eq!(s.receive(&vector("m3")), Err(WpaError::Replay));
    // The group message twice: the second is a replay; a new group message
    // with the same key is answered, not installed (tested via the parts).
    assert!(matches!(s.receive(&vector("g1")), Ok(Step::Rekey { .. })));
    assert_eq!(s.receive(&vector("g1")), Err(WpaError::Replay));
    // Message 1 after the handshake is not taken (no PTK rekey here).
    assert_eq!(s.receive(&vector("m1")), Err(WpaError::Unexpected));
}

#[test]
fn a_repeated_message_3_with_a_higher_counter_is_answered_without_keys() {
    let mut s = supplicant();
    s.receive(&vector("m1")).unwrap();
    s.receive(&vector("m3")).unwrap();
    let kck = vector("kck");
    let mut again = KeyFrame::parse(&vector("m3")).unwrap();
    again.replay += 5;
    let bytes = again.sign(&kck);
    match s.receive(&bytes).unwrap() {
        Step::Send(reply) => {
            assert_eq!(KeyFrame::parse(&reply).unwrap().replay, 7);
        }
        other => panic!("keys handed out twice: {other:?}"),
    }
}

#[test]
fn a_wrong_passphrase_fails_the_mic_of_message_3() {
    let pmk = Pmk::from_passphrase("not the password", &vector("ssid")).unwrap();
    let mut s = Supplicant::new(
        pmk,
        mac("aa"),
        mac("spa"),
        vector("rsn_ap"),
        supplicant_element(),
        vector("snonce").try_into().unwrap(),
    );
    assert!(matches!(s.receive(&vector("m1")), Ok(Step::Send(_))));
    assert_eq!(s.receive(&vector("m3")), Err(WpaError::BadMic));
    assert!(!s.complete());
}

#[test]
fn downgrade_changed_nonce_and_order_are_refused() {
    // Another RSN element than the beacon's: a downgrade.
    let pmk = Pmk::from_bytes(vector("pmk").try_into().unwrap());
    let mut s = Supplicant::new(
        pmk,
        mac("aa"),
        mac("spa"),
        supplicant_element(),
        supplicant_element(),
        vector("snonce").try_into().unwrap(),
    );
    s.receive(&vector("m1")).unwrap();
    assert_eq!(s.receive(&vector("m3")), Err(WpaError::RsnMismatch));
    // Message 3 before message 1, group key before the end.
    let mut s = supplicant();
    assert_eq!(s.receive(&vector("m3")), Err(WpaError::Unexpected));
    assert_eq!(s.receive(&vector("g1")), Err(WpaError::Unexpected));
    // Another ANonce in message 3.
    s.receive(&vector("m1")).unwrap();
    let mut m3 = KeyFrame::parse(&vector("m3")).unwrap();
    m3.nonce[0] ^= 1;
    assert_eq!(s.receive(&m3.to_bytes()), Err(WpaError::NonceMismatch));
    // A changed byte of the key data breaks the MIC.
    let mut bytes = vector("m3");
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    assert_eq!(s.receive(&bytes), Err(WpaError::BadMic));
    // Version 1 descriptors (TKIP) and frames without ACK are not answered.
    let mut m1 = KeyFrame::parse(&vector("m1")).unwrap();
    m1.info = (m1.info & !info::VERSION) | 1;
    assert_eq!(s.receive(&m1.to_bytes()), Err(WpaError::Unexpected));
    let mut m1 = KeyFrame::parse(&vector("m1")).unwrap();
    m1.info &= !info::ACK;
    assert_eq!(s.receive(&m1.to_bytes()), Err(WpaError::Unexpected));
    assert!(!format!("{s:?}").contains("pmk"));
}

#[test]
fn key_data_checks() {
    assert_eq!(unwrap(&[0; 16], &[0; 16]), Err(WpaError::BadKeyData));
    assert_eq!(unwrap(&[0; 16], &[0; 25]), Err(WpaError::BadKeyData));
    assert_eq!(unwrap(&[0; 16], &[0; 24]), Err(WpaError::BadKeyData));
    assert_eq!(gtk_kde(&[48, 0]), Err(WpaError::BadKeyData));
    let mut kde = vec![0xdd, 22, 0x00, 0x0f, 0xac, 0x01, 0x00, 0x00];
    kde.extend_from_slice(&[7; 16]);
    assert_eq!(gtk_kde(&kde), Err(WpaError::BadKeyData), "index 0");
    kde[6] = 0x06; // index 2, Tx bit
    assert_eq!(gtk_kde(&kde).unwrap().0, 2);
    kde[1] = 21;
    kde.pop();
    assert_eq!(gtk_kde(&kde), Err(WpaError::BadKeyData), "short key");
    for e in [
        WpaError::BadPassphrase,
        WpaError::BadSsid,
        WpaError::BadElement,
        WpaError::BadFrame,
        WpaError::Unexpected,
        WpaError::Replay,
        WpaError::BadMic,
        WpaError::NonceMismatch,
        WpaError::RsnMismatch,
        WpaError::BadKeyData,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn fuzzed_frames_never_panic() {
    let mut seed = 0x9e37_79b9u32;
    let base = [vector("m1"), vector("m3"), vector("g1")];
    for round in 0..3000 {
        let mut s = supplicant();
        if round % 2 == 1 {
            s.receive(&vector("m1")).unwrap();
        }
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let mut f = base[seed as usize % 3].clone();
        let at = (seed >> 4) as usize % f.len();
        f[at] ^= (seed >> 12) as u8 | 1;
        let _ = s.receive(&f);
    }
}
