use super::*;

const PRIV: &str = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
const PUB: &str = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";

fn good() -> String {
    format!(
        "# office\n[Interface]\nPrivateKey = {PRIV}\nAddress = 10.8.0.2/32, fd00::2/128\nDNS = 10.8.0.1\nMTU = 1420\n\n\
         [Peer]\nPublicKey = {PUB}\nPresharedKey = {PRIV}\nAllowedIPs = 0.0.0.0/0, ::/0\nEndpoint = vpn.example.org:51820\nPersistentKeepalive = 25\n"
    )
}

#[test]
fn a_standard_file_is_read() {
    let c = parse(&good()).unwrap();
    assert_eq!(c.addresses.len(), 2);
    assert_eq!(c.addresses[0], ("10.8.0.2".parse().unwrap(), 32));
    assert_eq!(c.dns, vec!["10.8.0.1".parse::<IpAddr>().unwrap()]);
    assert_eq!(c.mtu, Some(1420));
    assert_eq!(c.endpoint.to_string(), "vpn.example.org:51820");
    assert_eq!(c.keepalive, Some(25));
    assert!(c.full_tunnel());
    assert!(c.preshared.is_some());
    assert!(!format!("{c:?}").contains("yAnz"));
    let lower = good()
        .replace("PrivateKey", "privatekey")
        .replace("[Peer]", "[peer]");
    assert!(parse(&lower).is_ok());
    let v6 = good().replace("vpn.example.org:51820", "[2001:db8::1]:51820");
    assert_eq!(
        parse(&v6).unwrap().endpoint.to_string(),
        "[2001:db8::1]:51820"
    );
    let split = good().replace("0.0.0.0/0, ::/0", "10.0.0.0/8");
    assert!(!parse(&split).unwrap().full_tunnel());
}

fn check(cases: Vec<(String, Refusal)>) {
    for (text, want) in cases {
        assert_eq!(parse(&text).err(), Some(want), "{text}");
    }
}

#[test]
fn refusals_of_the_interface_name_their_line() {
    check(vec![
        (
            good().replace("MTU = 1420", "PostUp = rm -rf /"),
            (ConfProblem::UnknownKey, 6),
        ),
        (
            good().replace("MTU = 1420", "Table = off"),
            (ConfProblem::UnknownKey, 6),
        ),
        (
            good().replace("MTU = 1420", "MTU = 80"),
            (ConfProblem::BadNumber, 6),
        ),
        (
            good().replace("DNS = 10.8.0.1", "DNS = ten"),
            (ConfProblem::BadAddress, 5),
        ),
        (
            good().replace("10.8.0.2/32", "10.8.0.2/33"),
            (ConfProblem::BadAddress, 4),
        ),
        (good().replace("[Peer]", "[Pear]"), (ConfProblem::Syntax, 8)),
    ]);
    let many = good().replace("10.8.0.1", &vec!["1.1.1.1"; 65].join(","));
    assert_eq!(parse(&many).err(), Some((ConfProblem::BadAddress, 5)));
}

#[test]
fn refusals_of_the_peer_and_the_file_name_their_line() {
    check(vec![
        (good().replace(PUB, "c2hvcnQ="), (ConfProblem::BadKey, 9)),
        (good().replace(PUB, "not base64!"), (ConfProblem::BadKey, 9)),
        (
            good().replace("vpn.example.org:51820", "vpn.example.org"),
            (ConfProblem::BadEndpoint, 12),
        ),
        (
            good().replace("vpn.example.org:51820", "vpn.example.org:0"),
            (ConfProblem::BadEndpoint, 12),
        ),
        (
            good().replace("vpn.example.org:51820", "bad_host:1"),
            (ConfProblem::BadEndpoint, 12),
        ),
        (
            good().replace("vpn.example.org:51820", "[nope]:1"),
            (ConfProblem::BadEndpoint, 12),
        ),
        (
            good().replace("MTU = 1420", "MTU 1420"),
            (ConfProblem::Syntax, 6),
        ),
        (
            good().replace("MTU = 1420", "DNS = 1.1.1.1"),
            (ConfProblem::Duplicate, 6),
        ),
        (
            format!("{}\n[Peer]\n", good()),
            (ConfProblem::PeerCount, 15),
        ),
        (
            good().replace("Endpoint = vpn.example.org:51820\n", ""),
            (ConfProblem::Missing, 0),
        ),
        (
            good().replace("Address = 10.8.0.2/32, fd00::2/128\n", ""),
            (ConfProblem::Missing, 0),
        ),
        (
            good().split("[Peer]").next().unwrap().to_owned(),
            (ConfProblem::PeerCount, 0),
        ),
        (
            format!("Address = 1.2.3.4\n{}", good()),
            (ConfProblem::Syntax, 1),
        ),
        ("x".repeat(MAX_FILE + 1), (ConfProblem::Unreadable, 0)),
        (
            format!("{}\n# {}", good(), "y".repeat(MAX_LINE)),
            (ConfProblem::Unreadable, 15),
        ),
        ("a\0b".into(), (ConfProblem::Unreadable, 0)),
        (
            format!("{}[Interface]\n", good()),
            (ConfProblem::Syntax, 14),
        ),
    ]);
}

#[test]
fn fuzzed_files_never_panic() {
    let base = good().into_bytes();
    let mut seed = 0xc0ff_ee11u32;
    for _ in 0..3000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let mut t = base.clone();
        let at = seed as usize % t.len();
        t[at] = b"=[]#,:/ \nAz09+"[(seed >> 8) as usize % 14];
        if let Ok(text) = String::from_utf8(t) {
            let _ = parse(&text);
        }
    }
}
