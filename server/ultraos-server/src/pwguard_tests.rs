use super::*;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn a_stranger_locks_out_only_his_own_address() {
    let guard = PasswordGuard::with_bounds(3, 60_000, 100);
    let stranger = ip("203.0.113.9");
    for _ in 0..3 {
        assert!(guard.book(1, stranger, false, 0).is_ok());
    }
    assert_eq!(guard.book(1, stranger, false, 30_000), Err(30));
    // the same /64 is the same client; another address is not
    assert!(guard.book(1, ip("2001:db8::1"), false, 0).is_ok());
    assert!(guard.book(1, ip("2001:db8::ffff"), false, 0).is_ok());
    assert!(guard.book(1, ip("198.51.100.7"), false, 30_000).is_ok());
    assert!(
        guard.book(2, stranger, false, 30_000).is_ok(),
        "another account"
    );
    assert!(guard.book(1, stranger, false, 60_000).is_ok(), "new window");
}

#[test]
fn failures_of_many_clients_slow_the_account_down_before_they_stop_it() {
    let guard = PasswordGuard::with_bounds(1000, 60_000, 100_000);
    let mut delays = Vec::new();
    for n in 0..ACCOUNT_MAX_FAILURES {
        let client = IpAddr::from([10, 0, (n / 256) as u8, (n % 256) as u8]);
        delays.push(guard.book(1, client, false, 0).unwrap().delay);
    }
    assert!(delays[..10].iter().all(|d| d.is_zero()));
    assert_eq!(delays[10], Duration::from_secs(1));
    assert_eq!(delays[11], Duration::from_secs(2));
    assert_eq!(delays[14], MAX_DELAY);
    assert_eq!(delays[199], MAX_DELAY);
    let owner = ip("192.0.2.1");
    assert!(guard.book(1, owner, false, 1).is_err(), "account cap");
    // a managing device of the account is never stopped by others' failures
    let with_device = guard.book(1, owner, true, 1).unwrap();
    assert_eq!(with_device.delay, Duration::ZERO);
    assert!(guard.book(2, owner, false, 1).is_ok(), "another account");
}

#[test]
fn right_passwords_give_their_place_back() {
    let guard = PasswordGuard::with_bounds(2, 60_000, 10);
    let owner = ip("192.0.2.1");
    for _ in 0..20 {
        let b = guard.book(1, owner, false, 0).unwrap();
        guard.succeeded(&b);
    }
    assert_eq!(guard.tracked(), (0, 0));
    let first = guard.book(1, owner, false, 0).unwrap();
    let _second = guard.book(1, owner, true, 0).unwrap();
    guard.succeeded(&first);
    assert!(
        guard.book(1, owner, false, 0).is_ok(),
        "one failure and one place left"
    );
    assert!(guard.book(1, owner, false, 0).is_err());
    assert_eq!(
        guard.tracked().1,
        1,
        "the managing check was not booked on the account"
    );
}

#[test]
fn parallel_guesses_cannot_exceed_the_budget() {
    let guard = std::sync::Arc::new(PasswordGuard::with_bounds(5, 60_000, 10));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let guard = guard.clone();
            std::thread::spawn(move || guard.book(1, ip("192.0.2.1"), false, 0).is_ok())
        })
        .collect();
    let booked = threads
        .into_iter()
        .filter_map(|t| t.join().ok())
        .filter(|ok| *ok)
        .count();
    assert_eq!(booked, 5);
}

#[test]
fn a_full_table_never_refuses_it_drops_the_oldest_entry() {
    let guard = PasswordGuard::with_bounds(1, 1000, 2);
    assert!(guard.book(1, ip("192.0.2.1"), false, 0).is_ok());
    assert!(guard.book(2, ip("192.0.2.1"), false, 10).is_ok());
    // a third account while the table is full and nothing expired: still checked
    assert!(guard.book(3, ip("192.0.2.1"), false, 20).is_ok());
    assert_eq!(guard.tracked(), (2, 2));
    // account 1 was dropped (oldest): its client may try again; account 2 is still counted
    assert!(guard.book(1, ip("192.0.2.1"), false, 30).is_ok());
    assert!(guard.book(3, ip("192.0.2.1"), false, 30).is_err());
    assert_eq!(PasswordGuard::new(0).pair_max, 1);
}

#[test]
fn delays_grow_and_stop_growing() {
    assert_eq!(delay_for(0), Duration::ZERO);
    assert_eq!(delay_for(ACCOUNT_FREE - 1), Duration::ZERO);
    assert_eq!(delay_for(ACCOUNT_FREE), Duration::from_secs(1));
    assert_eq!(delay_for(ACCOUNT_FREE + 2), Duration::from_secs(4));
    assert_eq!(delay_for(u32::MAX), MAX_DELAY);
}
