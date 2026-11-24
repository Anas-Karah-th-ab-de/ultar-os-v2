//! Two state machines against each other (real crypto), plus hostile peers.

use super::*;

fn addr(last: u8, random: bool) -> Addr {
    Addr {
        bytes: [last, 0x11, 0x22, 0x33, 0x44, 0x55],
        random,
    }
}

struct Side {
    smp: Smp,
    asks: Vec<Ask>,
    done: Option<Keys>,
    failed: Option<u8>,
    encrypt: Option<Zeroizing<[u8; 16]>>,
}

fn pair(io_a: Io, io_b: Io) -> (Side, Side) {
    let (a, b) = (addr(1, false), addr(2, true));
    let side = |smp| Side {
        smp,
        asks: vec![],
        done: None,
        failed: None,
        encrypt: None,
    };
    (
        side(Smp::new(true, io_a, a, b)),
        side(Smp::new(false, io_b, b, a)),
    )
}

/// Delivers `outs` of `from` to `to` until quiet; returns PDUs sent by both in order.
fn run(a: &mut Side, b: &mut Side, first: Vec<Out>, tamper: &dyn Fn(&mut Vec<u8>)) -> Vec<Vec<u8>> {
    let mut log = Vec::new();
    let mut queue: Vec<(bool, Out)> = first.into_iter().map(|o| (true, o)).collect();
    while !queue.is_empty() {
        let (from_a, out) = queue.remove(0);
        let (src, dst) = if from_a {
            (&mut *a, &mut *b)
        } else {
            (&mut *b, &mut *a)
        };
        match out {
            Out::Send(mut pdu) => {
                tamper(&mut pdu);
                log.push(pdu.clone());
                queue.extend(dst.smp.pdu(&pdu, true).into_iter().map(|o| (!from_a, o)));
            }
            Out::Ask(ask) => src.asks.push(ask),
            Out::Encrypt(k) => src.encrypt = Some(k),
            Out::Done(k) => src.done = Some(k),
            Out::Failed(r) => src.failed = Some(r),
        }
    }
    log
}

fn answer(a: &mut Side, b: &mut Side, a_says: bool, b_says: bool) {
    let outs = b.smp.answer(b_says);
    run(b, a, outs, &|_| {});
    let outs = a.smp.answer(a_says);
    run(a, b, outs, &|_| {});
}

fn encrypt(a: &mut Side, b: &mut Side) {
    let ltk = a.encrypt.clone().expect("central encrypts");
    assert_eq!(
        b.smp.ltk().as_ref(),
        Some(&ltk),
        "the same LTK on both sides"
    );
    for s in [&mut *a, &mut *b] {
        let outs = s.smp.encrypted(true);
        for o in outs {
            if let Out::Done(k) = o {
                s.done = Some(k);
            }
        }
    }
}

#[test]
fn numeric_comparison_both_accept() {
    let (mut a, mut b) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
    let first = a.smp.start();
    let log = run(&mut a, &mut b, first, &|_| {});
    assert_eq!(log[0][0], 0x01);
    assert_eq!(log[0][3] & 0x0d, 0x0d, "bonding, MITM, Secure Connections");
    let (Some(Ask::Compare(x)), Some(Ask::Compare(y))) = (a.asks.last(), b.asks.last()) else {
        panic!("both compare: {:?} {:?}", a.asks, b.asks);
    };
    assert_eq!(x, y);
    assert!(*x < 1_000_000);
    answer(&mut a, &mut b, true, true);
    encrypt(&mut a, &mut b);
    let (ka, kb) = (a.done.unwrap(), b.done.unwrap());
    assert_eq!(ka.ltk, kb.ltk);
    assert!(ka.authenticated && kb.authenticated);
}

#[test]
fn numeric_comparison_rejected_on_either_side() {
    for (sa, sb) in [(false, true), (true, false)] {
        let (mut a, mut b) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
        let first = a.smp.start();
        run(&mut a, &mut b, first, &|_| {});
        answer(&mut a, &mut b, sa, sb);
        assert!(a.failed.is_some() && b.failed.is_some(), "{sa} {sb}");
        assert!(a.encrypt.is_none() && b.smp.ltk().is_none());
    }
}

#[test]
fn just_works_with_a_device_without_display() {
    let (mut a, mut b) = pair(Io::DisplayYesNo, Io::NoIo);
    let first = a.smp.start();
    run(&mut a, &mut b, first, &|_| {});
    assert_eq!(a.asks, vec![Ask::Consent], "the person still agrees");
    answer(&mut a, &mut b, true, true);
    encrypt(&mut a, &mut b);
    assert!(!a.done.unwrap().authenticated);
}

#[test]
fn passkey_shown_here_typed_on_the_keyboard() {
    for wrong in [false, true] {
        let (mut a, mut b) = pair(Io::DisplayYesNo, Io::KeyboardOnly);
        let first = a.smp.start();
        run(&mut a, &mut b, first, &|_| {});
        let Some(Ask::Show(key)) = a.asks.first().copied() else {
            panic!("{:?}", a.asks)
        };
        assert_eq!(b.asks, vec![Ask::Type]);
        let typed = if wrong { (key + 1) % 1_000_000 } else { key };
        let outs = b.smp.typed(typed);
        run(&mut b, &mut a, outs, &|_| {});
        if wrong {
            assert_eq!(a.failed, Some(reason::PASSKEY_FAILED));
            continue;
        }
        encrypt(&mut a, &mut b);
        assert!(a.done.unwrap().authenticated);
    }
}

#[test]
fn legacy_pairing_and_short_keys_are_refused() {
    for (byte, value, expected) in [
        (3, 0x05, reason::AUTH_REQUIREMENTS),
        (4, 7, reason::KEY_SIZE),
        (1, 9, reason::INVALID_PARAMS),
    ] {
        let (_, mut b) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
        let mut req = vec![0x01, 1, 0, 0x0d, 16, 0, 2];
        req[byte] = value;
        let outs = b.smp.pdu(&req, true);
        assert_eq!(
            outs,
            vec![Out::Send(vec![0x05, expected]), Out::Failed(expected)]
        );
    }
    let (_, mut b) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
    let outs = b.smp.pdu(&[0x01, 1, 0, 0x0d, 16, 0, 2], false);
    assert_eq!(outs[1], Out::Failed(reason::NOT_SUPPORTED), "not pairable");
}

#[test]
fn reflected_or_tampered_values_fail() {
    // The peripheral answers with the central's own public key.
    let (mut a, mut b) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
    let first = a.smp.start();
    let reflected = std::cell::RefCell::new(None::<Vec<u8>>);
    run(&mut a, &mut b, first, &|p| {
        if p[0] == 0x0C {
            let mut r = reflected.borrow_mut();
            match r.as_ref() {
                None => *r = Some(p.clone()),
                Some(first) => *p = first.clone(),
            }
        }
    });
    assert_eq!(a.failed, Some(reason::INVALID_PARAMS));
    // A changed confirm value.
    let (mut a, mut b) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
    let first = a.smp.start();
    run(&mut a, &mut b, first, &|p| {
        if p[0] == 0x03 {
            p[1] ^= 1
        }
    });
    assert_eq!(a.failed, Some(reason::CONFIRM_FAILED));
    // A changed DHKey check.
    let (mut a, mut b) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
    let first = a.smp.start();
    run(&mut a, &mut b, first, &|_| {});
    let outs = b.smp.answer(true);
    assert!(outs.is_empty());
    let outs = a.smp.answer(true);
    run(&mut a, &mut b, outs, &|p| {
        if p[0] == 0x0D {
            p[5] ^= 0x40
        }
    });
    assert!(b.failed == Some(reason::DHKEY_FAILED) || a.failed == Some(reason::DHKEY_FAILED));
    assert!(a.encrypt.is_none());
}

#[test]
fn hostile_pdus_never_panic_and_end_the_pairing() {
    let mut seed = 0x2545_f491_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..3000 {
        let (mut a, _) = pair(Io::DisplayYesNo, Io::DisplayYesNo);
        a.smp.start();
        let len = (next() % 70) as usize;
        let pdu: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let outs = a.smp.pdu(&pdu, true);
        assert!(outs
            .iter()
            .all(|o| !matches!(o, Out::Done(_) | Out::Encrypt(_))));
    }
}
