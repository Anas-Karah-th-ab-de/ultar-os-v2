//! Device identity end to end: the device client (`ultar-cloud`: key store, transcripts,
//! `DeviceHome`) against the real server. Finding S15 and `docs/DEVICE-IDENTITY.md`.

use futures_util::{SinkExt, StreamExt};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use tokio_tungstenite::tungstenite::Message;
use ultar_cloud::{
    authorize_replacement, signal_auth, signal_hello, Client, CloudError, DeviceHome, KeyStore,
};
use ultar_cloud_e2e::Stack;

fn works(client: &Client) -> bool {
    client.entitlements().is_ok()
}

/// Opens the signalling socket the way a device does; returns the last handshake frame.
fn signal(s: &Stack, home: &DeviceHome) -> serde_json::Value {
    let key = home.key_store().unwrap();
    let me = home.identity().unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/v1/signal", s.addr))
            .await
            .unwrap();
        ws.send(Message::Text(signal_hello(&key).unwrap()))
            .await
            .unwrap();
        let Some(Ok(Message::Text(challenge))) = ws.next().await else {
            panic!("no challenge");
        };
        let auth = match signal_auth(&key, &me.server, me.account_id, &challenge) {
            Ok(auth) => auth,
            Err(_) => return serde_json::from_str(&challenge).unwrap(),
        };
        ws.send(Message::Text(auth)).await.unwrap();
        let Some(Ok(Message::Text(last))) = ws.next().await else {
            panic!("no answer");
        };
        serde_json::from_str(&last).unwrap()
    })
}

#[test]
fn a_device_gets_its_own_key_id_and_number_and_keeps_them() {
    let s = Stack::start();
    let anna = s.signed_in("anna");
    let (phone, tablet) = (s.device_home("phone"), s.device_home("tablet"));

    let number = phone.register_number(&anna, "Phone", None).unwrap();
    assert!(number.starts_with("UOS-") && number.len() == 13);
    let me = phone.identity().unwrap();
    assert_eq!(me.number, number);
    assert_eq!(me.server, s.addr.to_string());
    assert_eq!(me.device_id, phone.device_id().unwrap());
    assert_eq!(phone.number().as_deref(), Some(number.as_str()));
    // Key and state are private files.
    for file in ["device.key", "device.state"] {
        let path = s.dir.path().join("devices/phone").join(file);
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // Asking again changes nothing; a second device (no serial numbers anywhere) is distinct.
    assert_eq!(phone.register_number(&anna, "Phone", None).unwrap(), number);
    let second = tablet.register_number(&anna, "Tablet", None).unwrap();
    assert_ne!(second, number);
    assert_ne!(tablet.device_id().unwrap(), me.device_id);

    let list = anna.devices().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(
        (list[0].name.as_str(), list[0].number.as_deref()),
        ("Phone", Some(number.as_str()))
    );
    assert_eq!(list[0].state, "active");
    assert!(!list[0].current, "an account token is no device");

    // Device token: issued against the key, usable for the account, names the device.
    let key = phone.key_store().unwrap();
    let session = anna.device_session(&key).unwrap();
    assert_eq!(
        (session.number.as_str(), session.device_id.as_str()),
        (number.as_str(), me.device_id.as_str())
    );
    let as_phone = anna.as_device(&key).unwrap();
    assert!(works(&as_phone));
    assert!(as_phone.devices().unwrap()[0].current);

    // Calls: the socket opens with the key and serves the device's number.
    assert_eq!(
        signal(&s, &phone),
        serde_json::json!({"type":"ready","number": number})
    );
}

#[test]
fn rename_remove_and_limit_through_the_client() {
    let s = Stack::start();
    let anna = s.signed_in("anna");
    let homes: Vec<DeviceHome> = (0..4).map(|i| s.device_home(&format!("d{i}"))).collect();
    for home in &homes[..3] {
        home.register_number(&anna, "", None).unwrap();
    }
    assert!(matches!(
        homes[3].register_number(&anna, "", None),
        Err(CloudError::DeviceLimit)
    ));
    assert_eq!(
        homes[3].identity(),
        None,
        "nothing stored for a refused registration"
    );

    let key0 = homes[0].key_store().unwrap();
    let id1 = homes[1].device_id().unwrap();
    anna.rename_device(&key0, &id1, "لوح أنس").unwrap();
    assert_eq!(anna.devices().unwrap()[1].name, "لوح أنس");
    removal_retires_the_device(&s, &anna, &homes);
}

/// Second half of the test above: device 1 is removed by device 0.
fn removal_retires_the_device(s: &Stack, anna: &Client, homes: &[DeviceHome]) {
    let key0 = homes[0].key_store().unwrap();
    let id1 = homes[1].device_id().unwrap();
    // Remove device 1: its token is dead at once, its key is retired, its socket refused.
    let key1 = homes[1].key_store().unwrap();
    let token1 = anna.as_device(&key1).unwrap();
    assert!(works(&token1));
    anna.remove_device(&key0, &id1).unwrap();
    assert!(!works(&token1), "token of the removed device still works");
    assert!(matches!(
        anna.device_session(&key1),
        Err(CloudError::DeviceRetired)
    ));
    assert!(matches!(
        homes[1].register_number(anna, "", None),
        Err(CloudError::DeviceRetired)
    ));
    assert_eq!(signal(s, &homes[1])["error"], "auth");
    // After a reset the device is a new device with a new id and number.
    let old_number = homes[1].number().unwrap();
    homes[1].reset().unwrap();
    s.clock.advance_millis(120_000);
    let new_number = homes[1].register_number(anna, "", None).unwrap();
    assert_ne!(
        new_number, old_number,
        "a retired number is never handed out again"
    );
    assert_ne!(homes[1].device_id().unwrap(), id1);

    let events: Vec<String> = anna
        .device_events(&key0, 0)
        .unwrap()
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(
        events,
        [
            "device_added",
            "device_added",
            "device_added",
            "device_removed",
            "device_added"
        ]
    );
}

#[test]
fn nobody_can_take_a_device_id_or_use_another_devices_key() {
    let s = Stack::start();
    let (victim, mallory) = (s.signed_in("victim"), s.signed_in("mallory"));
    let (phone, hers) = (
        s.device_home("victim-phone"),
        s.device_home("mallory-phone"),
    );
    let victim_id = phone.device_id().unwrap(); // the key exists, the device is not registered yet

    // Mallory "registers" first. Her own key yields her own id – never the victim's.
    hers.register_number(&mallory, "", None).unwrap();
    assert_ne!(hers.identity().unwrap().device_id, victim_id);
    assert!(mallory
        .devices()
        .unwrap()
        .iter()
        .all(|d| d.device_id != victim_id));
    // The victim is not blocked.
    let number = phone.register_number(&victim, "", None).unwrap();
    assert_eq!(phone.identity().unwrap().device_id, victim_id);
    // Even with a copy of the victim's whole state directory under her account: refused.
    let copy = s.dir.path().join("devices/stolen-copy");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::copy(
        s.dir.path().join("devices/victim-phone/device.key"),
        copy.join("device.key"),
    )
    .unwrap();
    std::fs::set_permissions(
        copy.join("device.key"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(matches!(
        DeviceHome::new(&copy).register_number(&mallory, "", None),
        Err(CloudError::DeviceTaken)
    ));
    // A password thief (account token only) cannot answer the victim's number.
    let thief_home = s.device_home("thief");
    let password_only = s.anonymous().login("victim", "password-123").unwrap();
    let stolen_account = Client::new(&s.base(), Some(password_only));
    let thief_number = thief_home
        .register_number(&stolen_account, "", None)
        .unwrap();
    assert_ne!(thief_number, number);
    assert_eq!(signal(&s, &thief_home)["number"], thief_number.as_str());
    assert_eq!(signal(&s, &phone)["number"], number.as_str());
}

#[test]
fn replacing_a_device_with_the_old_one_at_hand() {
    let s = Stack::start();
    let anna = s.signed_in("anna");
    let (old, new) = (s.device_home("old"), s.device_home("new"));
    let number = old.register_number(&anna, "Old", None).unwrap();
    let (old_key, new_key) = (old.key_store().unwrap(), new.key_store().unwrap());
    let old_state = old.identity().unwrap();

    let request = anna.replace_begin(&new_key).unwrap();
    // The old device refuses a request for another account or server …
    assert!(matches!(
        authorize_replacement(
            &old_key,
            &old_state.server,
            old_state.account_id + 1,
            &request
        ),
        Err(CloudError::ServerIdentityMismatch)
    ));
    // … and signs the genuine one.
    let grant =
        authorize_replacement(&old_key, &old_state.server, old_state.account_id, &request).unwrap();
    let done = anna
        .replace_finish(&new_key, &request, &grant, "New")
        .unwrap();
    assert_eq!(done.number, number);
    assert_eq!(done.device_id, new.device_id().unwrap());
    assert!(matches!(
        anna.device_session(&old_key),
        Err(CloudError::DeviceRetired)
    ));
    assert_eq!(anna.device_session(&new_key).unwrap().number, number);
    // The grant is used up: the old device is no longer a device of the account.
    assert!(matches!(
        anna.replace_finish(&new_key, &request, &grant, "New"),
        Err(CloudError::DeviceUnknown)
    ));
}

#[test]
fn replacing_a_lost_device_waits_and_can_be_cancelled() {
    let s = Stack::start();
    let anna = s.signed_in("anna");
    let (lost, other, new) = (
        s.device_home("lost"),
        s.device_home("other"),
        s.device_home("new"),
    );
    let number = lost.register_number(&anna, "", None).unwrap();
    other.register_number(&anna, "", None).unwrap();
    let lost_id = lost.device_id().unwrap();
    let (new_key, other_key) = (new.key_store().unwrap(), other.key_store().unwrap());

    assert!(matches!(
        anna.recover_start(&new_key, "wrong-password", &lost_id),
        Err(CloudError::InvalidCredentials)
    ));
    assert_eq!(
        anna.recover_start(&new_key, "password-123", &lost_id)
            .unwrap(),
        72 * 3600
    );
    let last = anna.device_events(&other_key, 0).unwrap().pop().unwrap();
    assert_eq!(last.event, "recovery_started");
    assert_eq!(last.device, lost_id[..8]);
    let early = anna.recover_finish(&new_key, &lost_id, "New").unwrap_err();
    assert_eq!(early.retry_after_secs(), Some(72 * 3600));
    // The other device stops it; a later attempt has to wait the whole time again.
    anna.recover_cancel(&other_key).unwrap();
    s.clock.advance_millis(73 * 3_600_000);
    assert!(matches!(
        anna.recover_finish(&new_key, &lost_id, "New"),
        Err(CloudError::RecoveryUnknown)
    ));
    anna.recover_start(&new_key, "password-123", &lost_id)
        .unwrap();
    s.clock.advance_millis(72 * 3_600_000);
    let done = anna.recover_finish(&new_key, &lost_id, "New").unwrap();
    assert_eq!(done.number, number);
    assert!(matches!(
        anna.device_session(&lost.key_store().unwrap()),
        Err(CloudError::DeviceRetired)
    ));
}

#[test]
fn password_change_ends_tokens_but_not_the_device() {
    let s = Stack::start();
    let anna = s.signed_in("anna");
    let phone = s.device_home("phone");
    let number = phone.register_number(&anna, "", None).unwrap();
    let key = phone.key_store().unwrap();
    let device_client = anna.as_device(&key).unwrap();
    let new_token = anna
        .change_password("password-123", "another-password-1")
        .unwrap();
    assert!(!works(&anna) && !works(&device_client));
    let anna = Client::new(&s.base(), Some(new_token));
    assert!(works(&anna));
    assert_eq!(anna.device_session(&key).unwrap().number, number);
    assert_eq!(signal(&s, &phone)["number"], number.as_str());
}

/// Puts a registration of the first release into the server's database (as the migration leaves it).
fn legacy_row(s: &Stack, account: &str, legacy_id: &str, number: &str) {
    let conn = s.state.db.lock();
    conn.execute(
        "INSERT INTO devices(account_id, device_id, state, created)
         SELECT id, 'legacy:' || ?2, 0, 0 FROM accounts WHERE username=?1",
        [account, legacy_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO numbers(number, account_id, device_row, state, created)
         SELECT ?2, account_id, id, 1, 0 FROM devices WHERE device_id = 'legacy:' || ?1",
        [legacy_id, number],
    )
    .unwrap();
    // The accounts' tokens were issued before the migration: move the recorded cut-off past them.
    conn.execute(
        "UPDATE meta SET value = CAST(strftime('%s','now') + 3600 AS TEXT) WHERE key='legacy_cutoff'",
        [],
    )
    .unwrap();
}

#[test]
fn an_updated_device_keeps_its_number_and_a_squatted_id_blocks_nobody() {
    let s = Stack::start();
    let (anna, mallory, bert) = (
        s.signed_in("anna"),
        s.signed_in("mallory"),
        s.signed_in("bert"),
    );
    legacy_row(&s, "anna", "a3f497b589a474e9", "UOS-AAAA-0001");
    // Mallory squatted the constant id of serial-less devices under the first release.
    legacy_row(&s, "mallory", "7af8c5b4c52e91a3", "UOS-BBBB-0002");

    // Anna's device as the update finds it: a number file, no key, no state.
    let phone = s.device_home("anna-phone");
    std::fs::create_dir_all(s.dir.path().join("devices/anna-phone")).unwrap();
    std::fs::write(
        s.dir.path().join("devices/anna-phone/number"),
        "UOS-AAAA-0001",
    )
    .unwrap();
    let kept = phone
        .register_number(&anna, "Phone", Some("a3f497b589a474e9"))
        .unwrap();
    assert_eq!(kept, "UOS-AAAA-0001", "upgraded in place");
    let list = anna.devices().unwrap();
    assert_eq!((list.len(), list[0].state.as_str()), (1, "active"));
    assert_eq!(list[0].device_id, phone.device_id().unwrap());
    assert_eq!(signal(&s, &phone)["number"], "UOS-AAAA-0001");

    // Bert's serial-less device computed the squatted id and was refused by the first release
    // (it has no number file). Now it simply registers.
    let berts = s.device_home("bert-phone");
    let number = berts
        .register_number(&bert, "", Some("7af8c5b4c52e91a3"))
        .unwrap();
    assert_ne!(number, "UOS-BBBB-0002");
    // Even a device that believes it owned the squatted registration cannot take it over.
    let confused = s.device_home("bert-tablet");
    std::fs::create_dir_all(s.dir.path().join("devices/bert-tablet")).unwrap();
    std::fs::write(
        s.dir.path().join("devices/bert-tablet/number"),
        "UOS-BBBB-0002",
    )
    .unwrap();
    let own = confused
        .register_number(&bert, "", Some("7af8c5b4c52e91a3"))
        .unwrap();
    assert!(
        own != "UOS-BBBB-0002" && own != number,
        "fell back to a fresh registration"
    );
    let hers = mallory.devices().unwrap();
    assert_eq!(
        (hers.len(), hers[0].number.as_deref()),
        (1, Some("UOS-BBBB-0002"))
    );
    assert_eq!(hers[0].state, "legacy", "untouched");
}

/// A server that relays another server's challenge gets no signature from the device.
#[test]
fn the_device_signs_only_for_the_server_it_is_configured_for() {
    let real = Stack::start();
    let anna = real.signed_in("anna");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let relay = format!("http://{}", listener.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (log, real_name) = (seen.clone(), real.addr.to_string());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            log.lock()
                .unwrap()
                .push(head.lines().next().unwrap_or("").to_string());
            // What the real server would answer: its own name in the challenge.
            let body = serde_json::json!({"challenge_id": "AAAAAAAAAAAAAAAAAAAAAA",
                "challenge": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "server": real_name, "account_id": 1, "expires_in": 120})
            .to_string();
            let _ = stream.write_all(
                format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes(),
            );
        }
    });
    let home = real.device_home("phone");
    let token = real.anonymous().login("anna", "password-123").unwrap();
    let through_relay = Client::new(&relay, Some(token));
    assert!(matches!(
        home.register_number(&through_relay, "", None),
        Err(CloudError::ServerIdentityMismatch)
    ));
    let requests = seen.lock().unwrap().clone();
    assert_eq!(
        requests,
        ["POST /v1/device/challenge HTTP/1.1"],
        "no signature was sent"
    );
    assert_eq!(home.identity(), None);
    // The same device registers normally at the real server.
    assert!(home.register_number(&anna, "", None).is_ok());
    let key = home.key_store().unwrap();
    assert_eq!(key.public_key().unwrap().len(), 32);
}
