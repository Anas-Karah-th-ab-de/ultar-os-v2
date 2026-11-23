//! Devices of an account: the limit per account and the lifetime quota, rename and remove (with
//! a device token only), retirement of removed keys and numbers, and what the server stores.

mod common;
use common::device::TestDevice;
use common::TestServer;
use serde_json::json;

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn device_count(s: &TestServer) -> i64 {
    s.state
        .db
        .lock()
        .query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn device_limit_is_configurable_and_enforced() {
    let s = TestServer::start_with(|c| c.devices.max_devices = 2);
    let token = s.register("anna");
    let first = TestDevice::new(1).register(&s, &token);
    TestDevice::new(2).register(&s, &token);
    let r = TestDevice::new(3).register_reply(&s, &token, "");
    assert_eq!(code(&r), (409, "device_limit".into()));
    assert_eq!(s.get_as("/v1/devices", &token).json()["limit"], 2);
    // an already registered key is still answered (no new device)
    assert_eq!(
        TestDevice::new(1).register(&s, &token)["number"],
        first["number"]
    );
    // removing one makes room; the lifetime quota (10 × limit) ends register/remove loops
    let device_token = first["token"].as_str().unwrap().to_string();
    let remove =
        |id: &str, with: &str| s.post("/v1/device/remove", Some(with), &json!({"device_id": id}));
    assert_eq!(remove(&TestDevice::new(2).id(), &device_token).status, 200);
    let mut last = (0, String::new());
    for seed in 3..=30u8 {
        s.clock.advance_millis(120_000); // used challenges count until they expire
        let d = TestDevice::new(seed);
        let r = d.register_reply(&s, &token, "");
        if r.status != 200 {
            last = code(&r);
            break;
        }
        assert_eq!(remove(&d.id(), &device_token).status, 200);
    }
    assert_eq!(last, (409, "device_quota_exhausted".into()));
    let rows = device_count(&s);
    assert_eq!(rows, 20, "rows stay bounded by the quota");
    // the default is 10
    let d = TestServer::start();
    let token = d.register("anna");
    for seed in 1..=10u8 {
        d.clock.advance_millis(120_000);
        TestDevice::new(seed).register(&d, &token);
    }
    d.clock.advance_millis(120_000);
    assert_eq!(
        code(&TestDevice::new(11).register_reply(&d, &token, "")),
        (409, "device_limit".into())
    );
}

#[test]
fn rename_and_remove_need_a_device_token_and_stay_inside_the_account() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (phone, tablet, foreign) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    let phone_token = phone.register(&s, &anna)["token"]
        .as_str()
        .unwrap()
        .to_string();
    let tablet_number = tablet.number(&s, &anna);
    let foreign_token = foreign.register(&s, &bert)["token"]
        .as_str()
        .unwrap()
        .to_string();

    let tokens = Tokens {
        anna,
        bert,
        phone_token,
        foreign_token,
    };
    rename_rules(&s, &tokens, &tablet);
    remove_rules(&s, &tokens, &tablet, &tablet_number);
}

/// The tokens the rename/remove test works with.
struct Tokens {
    anna: String,
    bert: String,
    phone_token: String,
    foreign_token: String,
}

fn rename_rules(s: &TestServer, t: &Tokens, tablet: &TestDevice) {
    let (anna, phone_token, foreign_token) = (&t.anna, &t.phone_token, &t.foreign_token);
    let rename = |with: Option<&str>, id: &str, name: &str| {
        s.post(
            "/v1/device/rename",
            with,
            &json!({"device_id": id, "name": name}),
        )
    };
    assert_eq!(
        rename(Some(phone_token), &tablet.id(), "لوح أنس").status,
        200
    );
    assert_eq!(
        s.get_as("/v1/devices", anna).json()["devices"][1]["name"],
        "لوح أنس"
    );
    // an account token (password) is not enough to change or remove devices
    assert_eq!(
        code(&rename(Some(anna), &tablet.id(), "x")),
        (401, "auth_required".into())
    );
    assert_eq!(
        code(&rename(None, &tablet.id(), "x")),
        (401, "auth_required".into())
    );
    assert_eq!(
        code(&rename(Some(foreign_token), &tablet.id(), "x")),
        (404, "device_unknown".into())
    );
    assert_eq!(
        code(&rename(Some(phone_token), &tablet.id(), "a\nb")),
        (400, "invalid_request".into())
    );
    assert_eq!(
        code(&rename(Some(phone_token), &tablet.id(), &"x".repeat(65))),
        (400, "invalid_request".into())
    );
}

fn remove_rules(s: &TestServer, t: &Tokens, tablet: &TestDevice, tablet_number: &str) {
    let (anna, bert) = (&t.anna, &t.bert);
    let (phone_token, foreign_token) = (&t.phone_token, &t.foreign_token);
    let remove =
        |with: Option<&str>, id: &str| s.post("/v1/device/remove", with, &json!({"device_id": id}));
    assert_eq!(
        code(&remove(Some(anna), &tablet.id())),
        (401, "auth_required".into())
    );
    assert_eq!(
        code(&remove(Some(foreign_token), &tablet.id())),
        (404, "device_unknown".into())
    );
    assert_eq!(
        remove(Some(phone_token), &tablet.id()).json()["removed"],
        true
    );
    assert_eq!(
        code(&remove(Some(phone_token), &tablet.id())),
        (404, "device_unknown".into())
    );
    let left = s.get_as("/v1/devices", anna).json();
    assert_eq!(left["devices"].as_array().unwrap().len(), 1);

    // The removed key is retired: it cannot come back, under no account; its number is not reused.
    assert_eq!(
        code(&tablet.register_reply(s, anna, "")),
        (410, "device_retired".into())
    );
    assert_eq!(
        code(&tablet.register_reply(s, bert, "")),
        (410, "device_retired".into())
    );
    assert_eq!(
        code(&tablet.challenge_reply(s, "session", None)),
        (410, "device_retired".into())
    );
    let conn = s.state.db.lock();
    let state: i64 = conn
        .query_row(
            "SELECT state FROM numbers WHERE number=?1",
            [tablet_number],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, 0, "retired, and still occupying its number");
}

#[test]
fn the_server_stores_only_key_id_number_and_coarse_times() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    device.register(&s, &token);
    device.session(&s);
    let conn = s.state.db.lock();
    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('devices') ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        columns,
        [
            "account_id",
            "created",
            "device_id",
            "epoch",
            "id",
            "last_seen",
            "name",
            "public_key",
            "state"
        ]
    );
    let (key, last_seen): (Vec<u8>, i64) = conn
        .query_row("SELECT public_key, last_seen FROM devices", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(key, device.public());
    assert!(last_seen > 0 && last_seen % 86_400 == 0);
    let event_device: String = conn
        .query_row("SELECT device FROM device_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        event_device,
        device.id()[..8],
        "notices carry a prefix only"
    );
}
