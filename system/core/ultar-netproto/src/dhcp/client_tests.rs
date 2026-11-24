use super::*;

const MAC: Mac = [2, 0, 0, 0, 0, 5];

fn server_reply(request: &[u8], kind: MessageType, extra: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let r = Message::parse(request).unwrap();
    let mut options = vec![
        (opt::TYPE, vec![kind as u8]),
        (opt::SERVER, vec![192, 168, 7, 1]),
        (opt::MASK, vec![255, 255, 255, 0]),
        (opt::ROUTER, vec![192, 168, 7, 1]),
        (
            opt::DNS,
            vec![
                192, 168, 7, 1, 0, 0, 0, 0, 9, 9, 9, 9, 8, 8, 8, 8, 1, 1, 1, 1,
            ],
        ),
        (opt::LEASE, 600u32.to_be_bytes().to_vec()),
    ];
    options.extend(extra.iter().cloned());
    Message {
        op: 2,
        xid: r.xid,
        flags: r.flags,
        ciaddr: r.ciaddr,
        yiaddr: Ipv4Addr::new(192, 168, 7, 50),
        chaddr: r.chaddr,
        options,
    }
    .to_bytes()
}

fn sent(a: &[Action]) -> Vec<u8> {
    match &a[0] {
        Action::Broadcast(b) | Action::Unicast(_, b) => b.clone(),
        other => panic!("{other:?}"),
    }
}

fn bound() -> (Client, Vec<u8>) {
    let mut c = Client::new(MAC, 42, 0);
    let discover = sent(&c.poll(0));
    let m = Message::parse(&discover).unwrap();
    assert_eq!(
        (m.kind(), m.flags),
        (Some(MessageType::Discover), BROADCAST)
    );
    let request = sent(&c.receive(&server_reply(&discover, MessageType::Offer, &[]), 10));
    let m = Message::parse(&request).unwrap();
    assert_eq!(m.kind(), Some(MessageType::Request));
    assert_eq!(
        m.address(opt::REQUESTED),
        Some(Ipv4Addr::new(192, 168, 7, 50))
    );
    let ack = server_reply(&request, MessageType::Ack, &[]);
    let actions = c.receive(&ack, 20);
    match &actions[0] {
        Action::Configure(l) => {
            assert_eq!(l.address, Ipv4Addr::new(192, 168, 7, 50));
            assert_eq!(l.prefix, 24);
            assert_eq!(l.router, Some(Ipv4Addr::new(192, 168, 7, 1)));
            assert_eq!(l.dns.len(), 3, "0.0.0.0 dropped, at most 3");
            assert_eq!(l.seconds, 600);
        }
        other => panic!("{other:?}"),
    }
    (c, ack)
}

#[test]
fn discover_offer_request_ack() {
    let (c, _) = bound();
    assert_eq!(c.next_deadline(), 20 + 300_000, "renewal at half the lease");
    assert!(c.lease().is_some());
}

#[test]
fn retransmissions_back_off_and_requests_start_over() {
    let mut c = Client::new(MAC, 1, 0);
    assert_eq!(c.poll(0).len(), 1);
    assert!(c.poll(3_999).is_empty());
    assert_eq!(c.poll(4_000).len(), 1);
    assert_eq!(c.next_deadline(), 12_000);
    let discover = sent(&c.poll(12_000));
    let mut now = 12_000;
    c.receive(&server_reply(&discover, MessageType::Offer, &[]), now);
    for _ in 0..3 {
        now = c.next_deadline();
        assert!(matches!(c.poll(now)[0], Action::Broadcast(_)));
    }
    now = c.next_deadline();
    assert!(c.poll(now).is_empty(), "back to selecting");
    let d = sent(&c.poll(now));
    assert_eq!(
        Message::parse(&d).unwrap().kind(),
        Some(MessageType::Discover)
    );
}

#[test]
fn renew_rebind_and_expire() {
    let (mut c, _) = bound();
    let renew = c.poll(300_020);
    match &renew[0] {
        Action::Unicast(server, b) => {
            assert_eq!(*server, Ipv4Addr::new(192, 168, 7, 1));
            let m = Message::parse(b).unwrap();
            assert_eq!(m.ciaddr, Ipv4Addr::new(192, 168, 7, 50));
            assert_eq!(m.flags, 0);
        }
        other => panic!("{other:?}"),
    }
    // The same lease again: nothing to configure.
    let ack = server_reply(&sent(&renew), MessageType::Ack, &[]);
    assert!(c.receive(&ack, 300_100).is_empty());
    assert_eq!(c.next_deadline(), 600_100);
    // No answer: renew until T2, rebind (broadcast) until the end.
    let mut now = 600_100;
    let mut kinds = Vec::new();
    while c.lease().is_some() {
        let a = c.poll(now);
        if let Some(first) = a.first() {
            kinds.push(match first {
                Action::Unicast(..) => "u",
                Action::Broadcast(_) => "b",
                Action::Deconfigure => "d",
                Action::Configure(_) => "c",
            });
        }
        now = c.next_deadline().max(now + 1);
    }
    assert_eq!(kinds.first(), Some(&"u"));
    assert!(kinds.contains(&"b"));
    assert_eq!(kinds.last(), Some(&"d"));
}

#[test]
fn nak_foreign_and_bad_offers() {
    let (mut c, _) = bound();
    let renew = sent(&c.poll(300_020));
    assert_eq!(
        c.receive(&server_reply(&renew, MessageType::Nak, &[]), 300_030),
        vec![Action::Deconfigure]
    );
    assert!(c.lease().is_none());
    let mut c = Client::new(MAC, 9, 0);
    let discover = sent(&c.poll(0));
    let mut other = Message::parse(&server_reply(&discover, MessageType::Offer, &[])).unwrap();
    other.xid ^= 1;
    assert!(
        c.receive(&other.to_bytes(), 1).is_empty(),
        "another transaction"
    );
    let mut other = Message::parse(&server_reply(&discover, MessageType::Offer, &[])).unwrap();
    other.chaddr[5] ^= 1;
    assert!(c.receive(&other.to_bytes(), 1).is_empty(), "another client");
    let mut bad = Message::parse(&server_reply(&discover, MessageType::Offer, &[])).unwrap();
    bad.yiaddr = Ipv4Addr::BROADCAST;
    assert!(c.receive(&bad.to_bytes(), 1).is_empty(), "unusable address");
    let mut bad = Message::parse(&server_reply(&discover, MessageType::Offer, &[])).unwrap();
    bad.options.retain(|(o, _)| *o != opt::MASK);
    bad.options.push((opt::MASK, vec![255, 0, 255, 0]));
    assert!(
        c.receive(&bad.to_bytes(), 1).is_empty(),
        "a mask with holes"
    );
    let mut no_mask = Message::parse(&server_reply(&discover, MessageType::Offer, &[])).unwrap();
    no_mask
        .options
        .retain(|(o, _)| *o != opt::MASK && *o != opt::LEASE);
    let request = sent(&c.receive(&no_mask.to_bytes(), 1));
    let mut ack = Message::parse(&server_reply(&request, MessageType::Ack, &[])).unwrap();
    ack.options
        .retain(|(o, _)| *o != opt::MASK && *o != opt::LEASE);
    match &c.receive(&ack.to_bytes(), 2)[0] {
        Action::Configure(l) => assert_eq!((l.prefix, l.seconds), (24, MIN_LEASE)),
        other => panic!("{other:?}"),
    }
    assert!(c.receive(&[1, 2, 3], 3).is_empty());
    let (r, bytes) = c.release().unwrap();
    assert_eq!(r, Ipv4Addr::new(192, 168, 7, 1));
    assert_eq!(
        Message::parse(&bytes).unwrap().kind(),
        Some(MessageType::Release)
    );
    assert_eq!(prefix_of(Ipv4Addr::new(255, 255, 255, 255)), Some(32));
    assert_eq!(classful(Ipv4Addr::new(10, 1, 1, 1)), 8);
    assert_eq!(classful(Ipv4Addr::new(172, 16, 1, 1)), 16);
}
