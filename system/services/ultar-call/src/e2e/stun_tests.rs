use super::*;

#[test]
fn messages_round_trip_with_padding() {
    let m = Msg::new(method::ALLOCATE, Class::Request)
        .with(attr::TRANSPORT, vec![17, 0, 0, 0])
        .with(attr::DATA, b"abcde".to_vec());
    let b = m.encode(None);
    assert_eq!(b.len() % 4, 0);
    assert_eq!(Msg::decode(&b).unwrap(), m);
    assert_eq!(Msg::decode(&b[..19]), None);
    assert_eq!(
        Msg::decode(b"\x40\x00\x00\x04abcd"),
        None,
        "ChannelData is no STUN"
    );
}

#[test]
fn classes_and_error_codes_are_read() {
    let mut m = Msg::new(method::ALLOCATE, Class::Error).with(attr::ERROR, vec![0, 0, 4, 1]);
    let d = Msg::decode(&m.encode(None)).unwrap();
    assert_eq!(
        (d.class, d.method, d.error_code()),
        (Class::Error, method::ALLOCATE, Some(401))
    );
    m.class = Class::Indication;
    m.method = method::DATA;
    let d = Msg::decode(&m.encode(None)).unwrap();
    assert_eq!((d.class, d.method), (Class::Indication, method::DATA));
}

#[test]
fn integrity_covers_the_message_with_the_right_key() {
    let auth = Auth {
        user: "1700000000:ab".into(),
        realm: "ultar.code-craft.tech".into(),
        nonce: "n1".into(),
        password: "pw".into(),
    };
    let b = Msg::new(method::REFRESH, Class::Request).encode(Some(&auth));
    let d = Msg::decode(&b).unwrap();
    let mi = d.get(attr::INTEGRITY).unwrap();
    // recompute over everything before the attribute, with the length that includes it
    let cut = b.len() - 24;
    let key = md5::compute("1700000000:ab:ultar.code-craft.tech:pw").0;
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(&key).unwrap();
    mac.update(&b[..cut]);
    assert_eq!(mi, &mac.finalize().into_bytes()[..]);
    assert_eq!(d.get(attr::USERNAME), Some(&b"1700000000:ab"[..]));
}

#[test]
fn addresses_and_channel_data() {
    let a: SocketAddr = "194.62.29.64:42007".parse().unwrap();
    assert_eq!(read_xor_addr(&xor_addr(a)), Some(a));
    assert_eq!(read_xor_addr(&[0, 2, 0, 0]), None);
    let f = channel_data(0x4001, b"sealed");
    assert_eq!(read_channel_data(&f), Some((0x4001, &b"sealed"[..])));
    assert_eq!(read_channel_data(&f[..5]), None);
    assert_eq!(read_channel_data(b"\x00\x01\x00\x00"), None);
}
