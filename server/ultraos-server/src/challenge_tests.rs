use super::*;

const PK: [u8; 32] = [7; 32];

#[test]
fn a_challenge_is_accepted_once_for_its_purpose_and_account() {
    let store = ChallengeStore::default();
    let c = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    let bound = store
        .take(&c.id, ChallengeUse::Register, Some(1), 10)
        .unwrap();
    assert_eq!((bound.account_id, bound.public_key), (1, PK));
    assert_eq!(bound.challenge, c.challenge);
    assert_eq!(
        store.take(&c.id, ChallengeUse::Register, Some(1), 20),
        Err(ChallengeError::Used)
    );
    assert_eq!(
        store.take(&[0; 16], ChallengeUse::Register, Some(1), 20),
        Err(ChallengeError::Unknown)
    );
}

#[test]
fn challenges_are_random_and_distinct() {
    let store = ChallengeStore::default();
    let a = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    let b = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    assert_ne!(a.challenge, b.challenge);
    assert_ne!(a.id, b.id);
    assert_ne!(a.challenge, [0; 32]);
}

#[test]
fn wrong_account_or_purpose_is_a_mismatch_and_consumes_the_challenge() {
    let store = ChallengeStore::default();
    let c = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    assert_eq!(
        store.take(&c.id, ChallengeUse::Register, Some(2), 1),
        Err(ChallengeError::Mismatch)
    );
    assert_eq!(
        store.take(&c.id, ChallengeUse::Register, Some(1), 2),
        Err(ChallengeError::Used),
        "the failed attempt used it up"
    );
    let c = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    assert_eq!(
        store.take(&c.id, ChallengeUse::Replace, Some(1), 1),
        Err(ChallengeError::Mismatch)
    );
    let c = store.issue(ChallengeUse::Session, 5, PK, 0).unwrap();
    assert_eq!(
        store
            .take(&c.id, ChallengeUse::Session, None, 1)
            .unwrap()
            .account_id,
        5
    );
}

#[test]
fn expiry_is_exact_and_expired_entries_are_forgotten() {
    let store = ChallengeStore::default();
    let a = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    let b = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    assert!(store
        .take(&a.id, ChallengeUse::Register, Some(1), CHALLENGE_TTL_MS - 1)
        .is_ok());
    assert_eq!(
        store.take(&b.id, ChallengeUse::Register, Some(1), CHALLENGE_TTL_MS),
        Err(ChallengeError::Expired)
    );
    assert_eq!(store.len(), 2);
    store
        .issue(ChallengeUse::Register, 2, PK, CHALLENGE_TTL_MS)
        .unwrap();
    assert_eq!(store.len(), 1, "issuing purges what has expired");
    assert_eq!(
        store.take(&a.id, ChallengeUse::Register, Some(1), CHALLENGE_TTL_MS),
        Err(ChallengeError::Unknown)
    );
}

#[test]
fn per_owner_and_total_bounds_hold_and_free_up_after_expiry() {
    let store = ChallengeStore::new(1000, 2, 3);
    store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
    store.issue(ChallengeUse::Upgrade, 1, PK, 100).unwrap();
    assert_eq!(
        store.issue(ChallengeUse::Register, 1, PK, 200),
        Err(ChallengeError::Capacity {
            retry_after_secs: 1
        })
    );
    // Sessions are counted per key, not per account.
    store.issue(ChallengeUse::Session, 1, PK, 300).unwrap();
    assert!(matches!(
        store.issue(ChallengeUse::Register, 2, PK, 400),
        Err(ChallengeError::Capacity { .. })
    ));
    assert_eq!(store.len(), 3);
    // After the lifetime of the first two, their owner has room again.
    store.issue(ChallengeUse::Register, 1, PK, 1100).unwrap();
    store.issue(ChallengeUse::Register, 2, PK, 1100).unwrap();
    assert_eq!(store.len(), 3);
    assert!(!store.is_empty());
    assert_eq!(store.ttl_secs(), 1);
}

#[test]
fn a_used_challenge_frees_its_slot_at_once_and_is_still_known_as_used() {
    let store = ChallengeStore::new(1000, 1, 10);
    for round in 0..5 {
        let c = store.issue(ChallengeUse::Register, 1, PK, round).unwrap();
        store
            .take(&c.id, ChallengeUse::Register, Some(1), round)
            .unwrap();
        assert_eq!(
            store.take(&c.id, ChallengeUse::Register, Some(1), round),
            Err(ChallengeError::Used)
        );
    }
    // A refused presentation frees the slot too; an unused one keeps it.
    let c = store.issue(ChallengeUse::Register, 1, PK, 10).unwrap();
    assert_eq!(
        store.take(&c.id, ChallengeUse::Register, Some(2), 11),
        Err(ChallengeError::Mismatch)
    );
    store.issue(ChallengeUse::Register, 1, PK, 12).unwrap();
    assert!(store.issue(ChallengeUse::Register, 1, PK, 13).is_err());
}

#[test]
fn the_wait_is_that_of_the_owner_not_of_the_whole_store() {
    let store = ChallengeStore::new(100_000, 1, 10);
    store.issue(ChallengeUse::Register, 7, PK, 0).unwrap();
    store.issue(ChallengeUse::Register, 1, PK, 60_000).unwrap();
    assert_eq!(
        store.issue(ChallengeUse::Register, 1, PK, 70_000),
        Err(ChallengeError::Capacity {
            retry_after_secs: 90
        })
    );
}

#[test]
fn session_requests_of_a_stranger_cannot_lock_a_key_out() {
    let store = ChallengeStore::new(1000, 2, 100);
    // A stranger who knows the public key keeps asking.
    let first = store.issue(ChallengeUse::Session, 1, PK, 0).unwrap();
    store.issue(ChallengeUse::Session, 1, PK, 1).unwrap();
    store.issue(ChallengeUse::Session, 1, PK, 2).unwrap();
    assert_eq!(store.len(), 2, "the oldest pending one was dropped");
    assert_eq!(
        store.take(&first.id, ChallengeUse::Session, None, 3),
        Err(ChallengeError::Unknown)
    );
    // The device asks in between and uses its challenge at once.
    let own = store.issue(ChallengeUse::Session, 1, PK, 4).unwrap();
    assert!(store.take(&own.id, ChallengeUse::Session, None, 5).is_ok());
    // The total bound still refuses.
    let small = ChallengeStore::new(1000, 8, 2);
    small.issue(ChallengeUse::Session, 1, [1; 32], 0).unwrap();
    small.issue(ChallengeUse::Session, 1, [2; 32], 0).unwrap();
    assert!(small.issue(ChallengeUse::Session, 1, [3; 32], 0).is_err());
}

#[test]
fn purposes_parse() {
    for (text, expected) in [
        ("register", ChallengeUse::Register),
        ("session", ChallengeUse::Session),
        ("upgrade", ChallengeUse::Upgrade),
        ("replace", ChallengeUse::Replace),
        ("recover", ChallengeUse::Recover),
        ("approve", ChallengeUse::Approve),
    ] {
        assert_eq!(ChallengeUse::parse(text), Some(expected));
    }
    assert_eq!(ChallengeUse::parse("signal"), None);
    assert_eq!(ChallengeUse::parse(""), None);
    assert!(ChallengeError::Used.to_string().contains("used"));
}
