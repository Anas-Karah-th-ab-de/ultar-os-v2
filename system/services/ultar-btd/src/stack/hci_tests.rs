//! Events in the byte layouts of Core Vol 4 Part E 7.7, written by hand (no controller is
//! reachable here to record a btmon trace from), and hostile input.

use super::*;

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn addresses_print_and_parse() {
    let a = Addr {
        bytes: [0x66, 0x55, 0x44, 0x33, 0x22, 0x11],
        random: true,
    };
    assert_eq!(a.text(), "11:22:33:44:55:66/r");
    assert_eq!(Addr::parse(&a.text()), Some(a));
    for bad in [
        "",
        "11:22:33:44:55",
        "11:22:33:44:55:6G",
        "11:22:33:44:55:666",
        "x11:22:33:44:55:66",
    ] {
        assert_eq!(Addr::parse(bad), None, "{bad}");
    }
    assert_eq!(a.smp_value(), [1, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
}

#[test]
fn command_and_acl_framing() {
    assert_eq!(command(op::RESET, &[]), hex("01 030c 00"));
    assert_eq!(command(op::WRITE_SCAN_ENABLE, &[0]), hex("01 1a0c 01 00"));
    assert_eq!(acl(0x0040, true, &[1, 2]), hex("02 4020 0200 0102"));
    assert_eq!(
        acl_in(&hex("4010 0100 09")).unwrap(),
        (0x40, false, &[9u8][..])
    );
    assert!(acl_in(&hex("4020 0500 01")).is_err());
}

#[test]
fn recorded_events() {
    assert_eq!(
        event(&hex("0e 0a 01 0910 00 665544332211")).unwrap(),
        Event::CommandComplete {
            opcode: op::READ_BD_ADDR,
            status: 0,
            data: hex("665544332211")
        }
    );
    assert_eq!(
        event(&hex("0f 04 00 01 0104")).unwrap(),
        Event::CommandStatus {
            opcode: op::INQUIRY,
            status: 0
        }
    );
    // Extended inquiry result: a keyboard (class 0x002540), RSSI -60, name "K380".
    let mut eir = hex("2f ff 01 665544332211 01 00 402500 3412 c4 05 09 4b333830");
    eir.resize(2 + 255, 0);
    let Event::Seen(seen) = event(&eir).unwrap() else {
        panic!()
    };
    assert_eq!(seen[0].class, 0x002540);
    assert_eq!(seen[0].rssi, Some(-60));
    assert_eq!(adv(&seen[0].data).name.as_deref(), Some("K380"));
}

#[test]
fn recorded_le_and_pairing_events() {
    // LE advertising report: connectable, random address, flags + HID uuid + appearance + name.
    let rep = hex("3e 1e 02 01 00 01 c6c5c4c3c2c1 12 020106 03031218 03190104 06094d6f757365 b0");
    let Event::Seen(seen) = event(&rep).unwrap() else {
        panic!()
    };
    assert!(seen[0].le && seen[0].connectable && seen[0].addr.random);
    assert_eq!(seen[0].rssi, Some(-80));
    let a = adv(&seen[0].data);
    assert_eq!(
        (a.name.as_deref(), a.appearance, a.hid),
        (Some("Mouse"), Some(0x0401), true)
    );
    assert_eq!(
        event(&hex(
            "3e 13 01 00 4000 00 01 c6c5c4c3c2c1 2400 0000 c800 00"
        ))
        .unwrap(),
        Event::LeConnected {
            status: 0,
            handle: 0x40,
            central: true,
            addr: Addr {
                bytes: [0xc6, 0xc5, 0xc4, 0xc3, 0xc2, 0xc1],
                random: true
            }
        }
    );
    assert_eq!(
        event(&hex("33 0a 665544332211 40e20100")).unwrap(),
        Event::UserConfirm {
            addr: Addr::public([0x66, 0x55, 0x44, 0x33, 0x22, 0x11]),
            value: 123_456
        }
    );
    assert_eq!(
        event(&hex("07 0d 00 665544332211 4b3338300000")).unwrap(),
        Event::RemoteName {
            status: 0,
            addr: Addr::public([0x66, 0x55, 0x44, 0x33, 0x22, 0x11]),
            name: b"K380".to_vec()
        }
    );
}

#[test]
fn names_are_cleaned_data() {
    assert_eq!(clean_name(b"a\x07b\nc"), "abc");
    assert_eq!(clean_name(&[0xff, b'x']), "\u{fffd}x");
    assert!(clean_name(&[b'a'; 300]).len() <= 64);
    assert_eq!(
        adv(&hex("05 09 41 42")).name,
        None,
        "a cut record ends the walk"
    );
}

#[test]
fn hostile_events_never_panic() {
    let mut seed = 0x1234_5678_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..20_000 {
        let len = (next() % 64) as usize;
        let mut p: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        if p.len() > 2 {
            p[0] = [0x0e, 0x0f, 0x2f, 0x22, 0x3e, 0x13, 0x07, 0x18, 0x05][(p[0] % 9) as usize];
            p[1] %= p.len() as u8;
        }
        let _ = event(&p);
        let _ = acl_in(&p);
        let _ = adv(&p);
    }
}
