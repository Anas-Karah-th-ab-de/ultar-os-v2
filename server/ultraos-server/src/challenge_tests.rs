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
    assert_eq!(
        store.len(),
        1,
        "a: marker of a presentation; b: expired, nothing kept"
    );
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
    // Session challenges take no room at all.
    store.issue(ChallengeUse::Session, 1, PK, 300).unwrap();
    store.issue(ChallengeUse::Register, 2, PK, 400).unwrap();
    assert!(matches!(
        store.issue(ChallengeUse::Register, 3, PK, 400),
        Err(ChallengeError::Capacity { .. })
    ));
    assert_eq!(store.len(), 3);
    // After the lifetime of the first two, their owner has room again.
    store.issue(ChallengeUse::Register, 1, PK, 1100).unwrap();
    store.issue(ChallengeUse::Register, 3, PK, 1100).unwrap();
    assert_eq!(store.len(), 3);
    assert!(!store.is_empty());
    assert_eq!(store.ttl_secs(), 1);
}

#[test]
fn markers_of_presented_challenges_are_bounded_per_account_and_give_way_when_full() {
    let store = ChallengeStore::new(120_000, 2, 12);
    let mut ids = Vec::new();
    for round in 0..30 {
        let c = store.issue(ChallengeUse::Register, 1, PK, round).unwrap();
        let _ = store.take(&c.id, ChallengeUse::Register, Some(1), round);
        ids.push(c.id);
    }
    assert_eq!(
        store.len(),
        MAX_USED_PER_OWNER,
        "the account's oldest markers went"
    );
    // the newest replays are named, the oldest are simply unknown – refused either way
    assert_eq!(
        store.take(&ids[29], ChallengeUse::Register, Some(1), 40),
        Err(ChallengeError::Used)
    );
    assert_eq!(
        store.take(&ids[0], ChallengeUse::Register, Some(1), 40),
        Err(ChallengeError::Unknown)
    );
    // a full store makes room by dropping markers, never pending challenges
    for account in 2..4 {
        for _ in 0..2 {
            store
                .issue(ChallengeUse::Register, account, PK, 50)
                .unwrap();
        }
    }
    assert_eq!(store.len(), 12);
    store.issue(ChallengeUse::Register, 9, PK, 60).unwrap();
    assert_eq!(store.len(), 12, "a marker gave way");
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
fn session_challenges_are_stateless_and_only_a_verified_presentation_uses_them() {
    let store = ChallengeStore::new(1000, 2, 100);
    // The device asks; a stranger who knows the public key asks again and again.
    let own = store.issue(ChallengeUse::Session, 1, PK, 0).unwrap();
    assert_eq!(own.id.len(), SESSION_ID_BYTES);
    for t in 1..50 {
        store.issue(ChallengeUse::Session, 1, PK, t).unwrap();
    }
    assert!(store.is_empty(), "issuing stores nothing");
    // Opening does not consume: a failed signature check leaves the challenge usable.
    let ticket = store
        .open_session(&own.id, ChallengeUse::Session, None, 60)
        .unwrap();
    assert_eq!((ticket.bound.account_id, ticket.bound.public_key), (1, PK));
    assert_eq!(ticket.bound.challenge, own.challenge);
    let again = store
        .open_session(&own.id, ChallengeUse::Session, None, 61)
        .unwrap();
    store.confirm(&again, 62).unwrap();
    assert_eq!(store.confirm(&ticket, 63), Err(ChallengeError::Used));
    assert_eq!(
        store.open_session(&own.id, ChallengeUse::Session, None, 64),
        Err(ChallengeError::Used)
    );
    // purpose, account, expiry, issue time in the future, other process
    let other = store.issue(ChallengeUse::Session, 1, PK, 100).unwrap();
    let check = |purpose, account, now| store.open_session(&other.id, purpose, account, now);
    assert_eq!(
        check(ChallengeUse::Register, None, 100),
        Err(ChallengeError::Mismatch)
    );
    assert_eq!(
        check(ChallengeUse::Session, Some(2), 100),
        Err(ChallengeError::Mismatch)
    );
    assert_eq!(
        check(ChallengeUse::Session, None, 1100),
        Err(ChallengeError::Expired)
    );
    assert_eq!(
        check(ChallengeUse::Session, None, 99),
        Err(ChallengeError::Unknown)
    );
    let elsewhere = ChallengeStore::new(1000, 2, 100);
    assert_eq!(
        elsewhere.take(&other.id, ChallengeUse::Session, None, 100),
        Err(ChallengeError::Unknown)
    );
    assert_eq!(
        store.take(&[1u8; 30], ChallengeUse::Session, None, 100),
        Err(ChallengeError::Unknown)
    );
}

#[test]
fn accepted_sessions_are_bounded_per_key_and_the_wait_is_the_keys_own() {
    let store = ChallengeStore::new(120_000, 2, 1000);
    for t in 0..MAX_SESSIONS_PER_KEY as u64 {
        let c = store.issue(ChallengeUse::Session, 1, PK, t * 1000).unwrap();
        store
            .take(&c.id, ChallengeUse::Session, None, t * 1000)
            .unwrap();
    }
    let c = store.issue(ChallengeUse::Session, 1, PK, 30_000).unwrap();
    assert_eq!(
        store.take(&c.id, ChallengeUse::Session, None, 30_000),
        Err(ChallengeError::Capacity {
            retry_after_secs: 90
        })
    );
    let other = store
        .issue(ChallengeUse::Session, 1, [8; 32], 30_000)
        .unwrap();
    assert!(store
        .take(&other.id, ChallengeUse::Session, None, 30_000)
        .is_ok());
    // still unused, it works once the oldest acceptance expired
    assert!(store
        .take(&c.id, ChallengeUse::Session, None, 120_000)
        .is_ok());
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
