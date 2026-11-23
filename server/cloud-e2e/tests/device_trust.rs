//! Review round 3, end to end through the device client: what a password alone can do, how the
//! owner is told about a pending move of his number, and how he stops it.

use ultar_cloud::{Client, CloudError, DeviceHome, SecurityWatch};
use ultar_cloud_e2e::Stack;

/// A client that holds what a thief of the password has: a fresh account token.
fn password_only(s: &Stack, user: &str) -> Client {
    let token = s.anonymous().login(user, "password-123").unwrap();
    Client::new(&s.base(), Some(token))
}

#[test]
fn a_password_thief_gets_a_device_of_his_own_and_nothing_of_the_owners() {
    let s = Stack::start();
    let owner = s.signed_in("anna");
    let (phone, thief_home) = (s.device_home("phone"), s.device_home("thief"));
    let number = phone.register_number(&owner, "Phone", None).unwrap();
    let thief = password_only(&s, "anna");
    let thief_number = thief_home.register_number(&thief, "", None).unwrap();
    assert_ne!(thief_number, number);
    let thief_key = thief_home.key_store().unwrap();
    let session = thief.device_session(&thief_key).unwrap();
    assert!(!session.manage);
    assert_eq!(session.manage_in, 72 * 3600);

    let phone_id = phone.device_id().unwrap();
    let wait = |e: CloudError| e.retry_after_secs();
    assert_eq!(
        thief.remove_device(&thief_key, &phone_id).map_err(wait),
        Err(Some(72 * 3600))
    );
    assert!(matches!(
        thief.rename_device(&thief_key, &phone_id, "mine"),
        Err(CloudError::NotApproved { .. })
    ));
    assert!(matches!(
        thief.device_events(&thief_key, 0),
        Err(CloudError::NotApproved { .. })
    ));
    assert!(matches!(
        thief.recover_cancel(&thief_key),
        Err(CloudError::NotApproved { .. })
    ));
    assert!(matches!(
        thief.approve_device(&thief_key, &phone_id),
        Err(CloudError::NotApproved { .. })
    ));
    // The owner sees the added device, marked as not managing, and removes it.
    let list = owner.account_devices().unwrap();
    assert_eq!(
        list.devices.iter().map(|d| d.manage).collect::<Vec<_>>(),
        [true, false]
    );
    let phone_key = phone.key_store().unwrap();
    owner
        .remove_device(&phone_key, &thief_home.device_id().unwrap())
        .unwrap();
    assert!(matches!(
        thief.device_session(&thief_key),
        Err(CloudError::DeviceRetired)
    ));
    assert_eq!(owner.device_session(&phone_key).unwrap().number, number);
}

#[test]
fn the_owners_second_device_is_approved_by_the_first() {
    let s = Stack::start();
    let owner = s.signed_in("anna");
    let (phone, tablet) = (s.device_home("phone"), s.device_home("tablet"));
    phone.register_number(&owner, "Phone", None).unwrap();
    tablet.register_number(&owner, "Tablet", None).unwrap();
    let (phone_key, tablet_key) = (phone.key_store().unwrap(), tablet.key_store().unwrap());
    let tablet_id = tablet.device_id().unwrap();
    assert!(!owner.device_session(&tablet_key).unwrap().manage);
    // An id that is not in the account's list is not approved.
    let unknown = s.device_home("unknown").device_id().unwrap();
    assert!(matches!(
        owner.approve_device(&phone_key, &unknown),
        Err(CloudError::DeviceUnknown)
    ));
    owner.approve_device(&phone_key, &tablet_id).unwrap();
    assert!(owner.device_session(&tablet_key).unwrap().manage);
    owner
        .rename_device(&tablet_key, &phone.device_id().unwrap(), "Old phone")
        .unwrap();
    let events: Vec<String> = owner
        .device_events(&tablet_key, 0)
        .unwrap()
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(events, ["device_added", "device_added", "device_approved"]);
}

/// Second half of the test below: ten hours later the phone asks, is told, and stops it.
fn told_and_stopped(s: &Stack, watch: &SecurityWatch, owner: &Client, phone_id: &str) {
    s.clock.advance_millis(10 * 3_600_000);
    let state = watch.check(owner).unwrap();
    let recovery = state.recovery.clone().unwrap();
    assert_eq!(
        (recovery.old_device.as_str(), state.recovery_hours()),
        (phone_id, Some(62))
    );
    assert!(state.needs_attention() && state.manage);
    assert_eq!(
        state
            .notices
            .iter()
            .filter(|n| n.event == "recovery_started")
            .count(),
        1
    );
    // Acknowledging the notices does not silence the pending move.
    watch.acknowledge(&state).unwrap();
    let again = watch.check(owner).unwrap();
    assert!(again.notices.is_empty() && again.recovery.is_some());

    watch.not_me(owner).unwrap();
    let after = watch.check(owner).unwrap();
    assert!(after.recovery.is_none());
    assert_eq!(
        after.notices.last().map(|n| n.event.as_str()),
        Some("recovery_cancelled")
    );
}

/// The recovery is started while the owner's phone is offline. When the phone asks, it is told –
/// for as long as the move is pending – and "this was not me" stops it.
#[test]
fn the_owner_is_told_about_a_pending_move_and_stops_it() {
    let s = Stack::start();
    let owner = s.signed_in("anna");
    let (phone, taker) = (s.device_home("phone"), s.device_home("taker"));
    let number = phone.register_number(&owner, "Phone", None).unwrap();
    let phone_id = phone.device_id().unwrap();
    let watch = SecurityWatch::new(DeviceHome::new(s.dir.path().join("devices/phone")));
    let quiet = watch.check(&owner).unwrap();
    assert!(quiet.recovery.is_none() && quiet.manage);
    watch.acknowledge(&quiet).unwrap();

    let thief = password_only(&s, "anna");
    let taker_key = taker.key_store().unwrap();
    assert_eq!(
        thief
            .recover_start(&taker_key, "password-123", &phone_id)
            .unwrap(),
        72 * 3600
    );
    // He floods the notice list with throw-away devices; the state is reported all the same.
    for round in 0..5 {
        let home = s.device_home(&format!("throwaway-{round}"));
        home.register_number(&thief, "", None).unwrap();
        thief
            .remove_device(&home.key_store().unwrap(), &home.device_id().unwrap())
            .unwrap();
    }
    told_and_stopped(&s, &watch, &owner, &phone_id);
    s.clock.advance_millis(80 * 3_600_000);
    assert!(matches!(
        thief.recover_finish(&taker_key, &phone_id, ""),
        Err(CloudError::RecoveryUnknown)
    ));
    let session = owner.device_session(&phone.key_store().unwrap()).unwrap();
    assert_eq!(session.number, number);
}

#[test]
fn password_guessing_through_any_path_locks_the_password_not_the_devices() {
    let s = Stack::start();
    let owner = s.signed_in("anna");
    let phone = s.device_home("phone");
    phone.register_number(&owner, "", None).unwrap();
    let key = phone.key_store().unwrap();
    let mut refused = None;
    for guess in 0..12 {
        match owner.change_password(&format!("guess-{guess}-xx"), "new-password-1") {
            Err(CloudError::InvalidCredentials) => {}
            other => {
                refused = Some((guess, other.err().and_then(|e| e.retry_after_secs())));
                break;
            }
        }
    }
    assert_eq!(
        refused,
        Some((10, Some(900))),
        "ten failed checks, then a 15 minute stop"
    );
    assert!(matches!(
        s.anonymous().login("anna", "password-123"),
        Err(CloudError::RateLimited { .. })
    ));
    assert!(
        owner.device_session(&key).is_ok(),
        "the device key still works"
    );
    s.clock.advance_millis(900_000);
    assert!(s.anonymous().login("anna", "password-123").is_ok());
}
