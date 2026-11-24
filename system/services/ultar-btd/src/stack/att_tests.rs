//! A HOGP keyboard+mouse as a trace of ATT PDUs (the layout of a common combo device:
//! GAP 0x0001-0x0007, GATT 0x0008-0x000b, battery 0x000c-0x000f, HID 0x0010-0x0030).

use super::*;

fn h(v: &[u8]) -> Vec<u8> {
    v.to_vec()
}

#[test]
fn a_combo_device_is_found_switched_to_boot_and_enabled() {
    let (mut c, first) = Hogp::start();
    assert_eq!(first, h(&[0x10, 1, 0, 0xff, 0xff, 0x00, 0x28]));
    // Two pages of primary services.
    let (send, _) = c.pdu(&[0x11, 6, 1, 0, 7, 0, 0x00, 0x18, 8, 0, 0x0b, 0, 0x01, 0x18]);
    assert_eq!(send, vec![h(&[0x10, 0x0c, 0, 0xff, 0xff, 0x00, 0x28])]);
    let (send, _) = c.pdu(&[
        0x11, 6, 0x0c, 0, 0x0f, 0, 0x0f, 0x18, 0x10, 0, 0x30, 0, 0x12, 0x18,
    ]);
    assert_eq!(send, vec![h(&[0x08, 0x10, 0, 0x30, 0, 0x03, 0x28])]);
    // Characteristics: protocol mode 0x12, boot kbd in 0x14 (CCCD 0x15), boot mouse in 0x18.
    let (send, _) = c.pdu(&[
        0x09, 7, 0x11, 0, 0x06, 0x12, 0, 0x4e, 0x2a, 0x13, 0, 0x12, 0x14, 0, 0x22, 0x2a, 0x17, 0,
        0x12, 0x18, 0, 0x33, 0x2a,
    ]);
    assert_eq!(send, vec![h(&[0x08, 0x18, 0, 0x30, 0, 0x03, 0x28])]);
    let (send, _) = c.pdu(&[0x01, 0x08, 0x18, 0, 0x0A]);
    assert_eq!(send[0], h(&[0x52, 0x12, 0, 0]), "boot protocol");
    assert_eq!(
        send[1],
        h(&[0x04, 0x15, 0, 0x16, 0]),
        "descriptors of the keyboard input"
    );
    let (send, _) = c.pdu(&[0x05, 1, 0x15, 0, 0x02, 0x29, 0x16, 0, 0x08, 0x29]);
    assert_eq!(send, vec![h(&[0x12, 0x15, 0, 1, 0])]);
    let (send, _) = c.pdu(&[0x13]);
    assert_eq!(send, vec![h(&[0x04, 0x19, 0, 0x30, 0])]);
    let (send, _) = c.pdu(&[0x05, 1, 0x19, 0, 0x02, 0x29]);
    assert_eq!(send, vec![h(&[0x12, 0x19, 0, 1, 0])]);
    let (_, notes) = c.pdu(&[0x13]);
    assert_eq!(
        notes,
        vec![Note::Ready {
            keyboard: true,
            mouse: true
        }]
    );
    let (_, notes) = c.pdu(&[0x1B, 0x14, 0, 0, 0, 4, 0, 0, 0, 0, 0]);
    assert_eq!(notes, vec![Note::Keyboard(vec![0, 0, 4, 0, 0, 0, 0, 0])]);
    let (_, notes) = c.pdu(&[0x1B, 0x18, 0, 1, 2, 3]);
    assert_eq!(notes, vec![Note::Mouse(vec![1, 2, 3])]);
    let (_, notes) = c.pdu(&[0x1B, 0x99, 0, 1]);
    assert!(notes.is_empty(), "unknown handles are dropped");
    // The peer's requests to our empty server.
    assert_eq!(c.pdu(&[0x02, 0x00, 0x02]).0, vec![h(&[0x03, 23, 0])]);
    assert_eq!(c.pdu(&[0x0A, 3, 0]).0, vec![h(&[0x01, 0x0A, 3, 0, 0x06])]);
}

#[test]
fn no_hid_service_or_refusals_end_it() {
    let (mut c, _) = Hogp::start();
    let (_, notes) = c.pdu(&[0x01, 0x10, 1, 0, 0x0A]);
    assert_eq!(notes, vec![Note::Failed(0x0A)]);
    let (mut c, _) = Hogp::start();
    c.pdu(&[0x11, 6, 1, 0, 0xff, 0xff, 0x12, 0x18]);
    let (_, notes) = c.pdu(&[0x01, 0x08, 1, 0, 0x05]);
    assert_eq!(
        notes,
        vec![Note::Failed(0x05)],
        "insufficient authentication"
    );
    let (mut c, _) = Hogp::start();
    let (_, notes) = c.pdu(&[0x11, 6, 5, 0, 1, 0, 0x12, 0x18]);
    assert_eq!(notes, vec![Note::Failed(0)], "end before start");
}

#[test]
fn hostile_pdus_never_panic() {
    let mut seed = 0x9e37_79b9_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..3000 {
        let (mut c, _) = Hogp::start();
        for _ in 0..4 {
            let len = (next() % 40) as usize;
            let mut pdu: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if let Some(op) = pdu.first_mut() {
                *op = [0x11, 0x09, 0x05, 0x13, 0x01, 0x1B][(*op % 6) as usize];
            }
            let _ = c.pdu(&pdu);
        }
    }
}
