//! Devices in different networks: they meet through a relay that follows the server's
//! rules, pair with a code and carry a file through it.
mod common;
use common::net::{dev, dir, linked, paired, until};
use std::time::Instant;
use ultar_bus::messages::mesh::{DeviceKind, SendOutcome};
use ultar_mesh::node::Effect;

mod stand_in {
    //! A relay that follows the server's rules: frames go to the device whose key the
    //! sender names, with the sender's key put in its place; a wrong token is refused.
    use std::collections::HashMap;
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tungstenite::Message;

    type Table = Arc<Mutex<HashMap<String, mpsc::Sender<Message>>>>;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn announce(table: &Table) {
        let table = table.lock().unwrap();
        let list: Vec<_> = table
            .keys()
            .map(|pk| serde_json::json!({ "pk": pk }))
            .collect();
        let text = serde_json::json!({ "typ": "geraete", "liste": list }).to_string();
        for tx in table.values() {
            let _ = tx.send(Message::Text(text.clone()));
        }
    }

    fn forward(table: &Table, own: &str, mut frame: Vec<u8>) -> Option<Message> {
        let target = hex(&frame[1..33]);
        let raw: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&own[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        let back = frame.clone();
        frame[1..33].copy_from_slice(&raw);
        let sent = table
            .lock()
            .unwrap()
            .get(&target)
            .is_some_and(|tx| tx.send(Message::Binary(frame)).is_ok());
        (!sent).then(|| {
            let mut unreachable = back[..41].to_vec();
            unreachable[0] = 4;
            Message::Binary(unreachable)
        })
    }

    fn client(table: Table, stream: TcpStream) {
        let Ok(mut socket) = tungstenite::accept(stream) else {
            return;
        };
        let Ok(Message::Text(login)) = socket.read() else {
            return;
        };
        let login: serde_json::Value = serde_json::from_str(&login).unwrap();
        if login["token"] != "good" {
            let _ = socket.send(Message::Text(
                r#"{"typ":"fehler","text":"anmeldung"}"#.into(),
            ));
            return;
        }
        let own = login["pk"].as_str().unwrap().to_owned();
        let (tx, rx) = mpsc::channel();
        tx.send(Message::Text(r#"{"typ":"bereit"}"#.into()))
            .unwrap();
        table.lock().unwrap().insert(own.clone(), tx);
        announce(&table);
        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        loop {
            for message in rx.try_iter() {
                if socket.send(message).is_err() {
                    break;
                }
            }
            match socket.read() {
                Ok(Message::Binary(frame)) if frame.len() >= 41 => {
                    if let Some(answer) = forward(&table, &own, frame) {
                        let _ = socket.send(answer);
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
                _ => {}
            }
        }
        table.lock().unwrap().remove(&own);
        announce(&table);
    }

    /// Starts the relay; its address.
    pub fn start() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/v1/verbund/relais", listener.local_addr().unwrap());
        let table = Table::default();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let table = table.clone();
                std::thread::spawn(move || client(table, stream));
            }
        });
        url
    }
}

fn login(url: &str, token: &str, name: &str) -> ultar_mesh::relay::Login {
    ultar_mesh::relay::Login {
        url: url.to_owned(),
        token: token.to_owned(),
        name: name.to_owned(),
        kind: "desktop".to_owned(),
    }
}

#[test]
fn devices_in_different_networks_meet_through_the_relay_and_hand_a_file_over() {
    let d = dir("relay");
    let url = stand_in::start();
    let mut phone = dev(&d, "phone", DeviceKind::Phone, None);
    let mut pc = dev(&d, "pc", DeviceKind::Desktop, None);
    let mut stranger = dev(&d, "stranger", DeviceKind::Desktop, None);
    phone.net.use_relay(login(&url, "good", "phone"));
    pc.net.use_relay(login(&url, "good", "pc"));
    stranger.net.use_relay(login(&url, "stolen", "stranger"));
    // Nobody knows anybody's address: the relay is the only way.
    until(&mut [&mut phone, &mut pc, &mut stranger], "met", |d| {
        linked(d[0], d[1]) && linked(d[1], d[0])
    });
    assert!(
        stranger.rt.node.state().peers.is_empty(),
        "a wrong token meets nobody"
    );
    // Met is not trusted: the code is still needed.
    assert!(!paired(&phone, &pc));
    let code = pc.rt.node.pair_begin(Instant::now()).code;
    let pc_id = pc.rt.node.id().to_owned();
    let (_, effects) = phone.rt.node.pair_join(&pc_id, &code);
    phone.rt.apply(effects);
    until(&mut [&mut phone, &mut pc], "paired", |d| {
        paired(d[0], d[1]) && paired(d[1], d[0])
    });

    let content: Vec<u8> = (0..400_000u32).map(|i| (i % 241) as u8).collect();
    let source = d.join("far.bin");
    std::fs::write(&source, &content).unwrap();
    let (outcome, effects) =
        phone
            .rt
            .node
            .send_file(&pc_id, source.to_str().unwrap(), Instant::now());
    assert!(
        matches!(outcome, SendOutcome::Offered { .. }),
        "{outcome:?}"
    );
    phone.rt.apply(effects);
    until(&mut [&mut phone, &mut pc], "asked", |d| {
        d[1].host.iter().any(|e| matches!(e, Effect::Incoming(_)))
    });
    let offer = pc.host.iter().find_map(|e| match e {
        Effect::Incoming(i) => Some(i.offer),
        _ => None,
    });
    let (_, effects) = pc.rt.node.answer(offer.unwrap(), true, Instant::now());
    let left = pc.rt.apply(effects);
    pc.host.extend(left);
    until(&mut [&mut phone, &mut pc], "arrived", |d| {
        d[1].host.iter().any(|e| matches!(e, Effect::Open { .. }))
    });
    assert_eq!(
        std::fs::read(d.join("inbox-pc").join("far.bin")).unwrap(),
        content
    );
    std::fs::remove_dir_all(d).unwrap();
}
