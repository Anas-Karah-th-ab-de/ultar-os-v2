//! Review round 4, observations: an added device that may not manage yet is a state the owner's
//! devices see for as long as it lasts; a socket gets notices from the moment its device is
//! approved or its cool-down is over, without signing in again.

mod common;
use common::device::{next_json, runtime, TestDevice};
use common::TestServer;
use serde_json::{json, Value};
use std::time::Duration;

#[test]
fn an_added_device_is_reported_until_it_is_approved() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let (phone, added) = (TestDevice::new(1), TestDevice::new(2));
    let first = phone.register(&s, &owner);
    assert_eq!(first["unapproved"], json!([]));
    added.register(&s, &owner);
    s.clock.advance_millis(3_600_000);
    let expected = json!([{"device": added.id(), "manage_in": 71 * 3600}]);
    assert_eq!(phone.session_reply(&s).json()["unapproved"], expected);
    assert_eq!(
        s.get_as("/v1/devices", &owner).json()["unapproved"],
        expected
    );
    // the added device does not list itself
    assert_eq!(added.session_reply(&s).json()["unapproved"], json!([]));
    phone.approve(&s, &added);
    assert_eq!(phone.session_reply(&s).json()["unapproved"], json!([]));
}

#[test]
fn the_report_ends_when_the_cool_down_is_over_or_the_device_is_removed() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let (phone, a, b) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    phone.register(&s, &owner);
    a.register(&s, &owner);
    s.clock.advance_millis(72 * 3_600_000 - 1000);
    b.register(&s, &owner);
    let waiting = |s: &TestServer| -> Vec<Value> {
        phone.session_reply(s).json()["unapproved"]
            .as_array()
            .unwrap()
            .clone()
    };
    assert_eq!(waiting(&s).len(), 2);
    s.clock.advance_millis(1000);
    assert_eq!(
        waiting(&s),
        vec![json!({"device": b.id(), "manage_in": 72 * 3600 - 1})]
    );
    let token = phone.session(&s);
    let body = json!({"device_id": b.id()});
    assert_eq!(s.post("/v1/device/remove", Some(&token), &body).status, 200);
    assert!(waiting(&s).is_empty());
}

async fn next_notice(ws: &mut common::device::Socket) -> Value {
    match tokio::time::timeout(Duration::from_millis(1500), next_json(ws)).await {
        Ok(frame) => frame,
        Err(_) => json!({"silent": true}),
    }
}

#[test]
fn a_connected_device_gets_notices_once_approved_or_once_its_wait_is_over() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let (phone, approved, waiting) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    phone.register(&s, &owner);
    approved.register(&s, &owner);
    waiting.register(&s, &owner);
    runtime().block_on(async {
        let (mut approved_ws, ready) = approved.signal(&s).await;
        assert_eq!(ready["manage"], false);
        let (mut waiting_ws, _) = waiting.signal(&s).await;
        phone.approve(&s, &approved);
        // The approval itself is the first notice the approved socket gets.
        assert_eq!(
            next_notice(&mut approved_ws).await["event"],
            "device_approved"
        );
        assert_eq!(next_notice(&mut waiting_ws).await, json!({"silent": true}));
        // The cool-down of the other one ends while it is connected.
        s.clock.advance_millis(72 * 3_600_000);
        TestDevice::new(4).register(&s, &owner);
        assert_eq!(next_notice(&mut waiting_ws).await["event"], "device_added");
        assert_eq!(next_notice(&mut approved_ws).await["event"], "device_added");
    });
}
