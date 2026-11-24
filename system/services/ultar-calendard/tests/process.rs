//! The real `ultar-calendard`: events per person, following who is in front.

use std::time::Duration;
use ultar_bus::messages::calendar::{Add, CalendarOutcome, Changed, Date, Day, Remove};
use ultar_bus::messages::session::SessionState;
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.calendard"
register = ["calendar"]
publish = ["calendar.changed"]
subscribe = ["session.state"]

[[rule]]
identity = "system.sessiond"
publish = ["session.state"]

[[rule]]
identity = "system.shell"
call = ["calendar.day"]
subscribe = ["calendar.changed"]

[[rule]]
identity = "app.calendar"
call = ["calendar.day", "calendar.add", "calendar.remove"]

[[rule]]
identity = "app.other"
call = []
"#;

const TODAY: Date = Date {
    year: 2026,
    month: 11,
    day: 24,
};

fn start(world: &World) -> Daemon {
    let dir = world.subdir("books");
    world.spawn(
        env!("CARGO_BIN_EXE_ultar-calendard"),
        "system.calendard",
        &["--dir", dir.to_str().unwrap()],
    )
}

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        // Right after kill -9 the broker may still route to the dead
        // connection (ServiceGone) until it sees it closed: ask again.
        !matches!(
            last,
            Err(BusError::Remote(
                ErrorCode::UnknownService | ErrorCode::ServiceGone
            ))
        )
    });
    last
}

fn front(world: &World, user: Option<&str>) {
    world
        .client("system.sessiond")
        .publish(&SessionState {
            user: user.map(str::to_owned),
            signed_in: user.iter().map(|u| (*u).to_owned()).collect(),
        })
        .unwrap();
}

fn add(title: &str, start: u16) -> Add {
    Add {
        date: TODAY,
        start_minute: start,
        duration_minutes: 45,
        title: title.into(),
    }
}

#[test]
fn events_belong_to_the_person_in_front() {
    let ids = [
        "system.calendard",
        "system.sessiond",
        "system.shell",
        "app.calendar",
        "app.other",
    ];
    let world = World::new("cal", POLICY, &ids);
    let daemon = start(&world);
    let app = world.client("app.calendar");
    let shell = world.client("system.shell");
    let changed = shell.subscribe::<Changed>().unwrap();
    assert_eq!(
        call(&app, &add("x", 1)).unwrap().outcome,
        CalendarOutcome::NoUser
    );
    front(&world, Some("amal"));
    changed.recv_timeout(WAIT).unwrap();
    assert_eq!(
        call(&app, &add("طبيب", 600)).unwrap().outcome,
        CalendarOutcome::Done(1)
    );
    assert_eq!(
        call(&app, &add("standup", 540)).unwrap().outcome,
        CalendarOutcome::Done(2)
    );
    assert_eq!(
        call(&app, &add("", 540)).unwrap().outcome,
        CalendarOutcome::Invalid
    );
    let day = call(&shell, &Day { date: TODAY }).unwrap();
    assert_eq!(
        day.events
            .iter()
            .map(|e| e.title.as_str())
            .collect::<Vec<_>>(),
        ["standup", "طبيب"]
    );
    switch_people_and_refusals(&world, &app, &shell);
    restart_and_damage(&world, daemon, &app, &shell);
}

fn restart_and_damage(world: &World, mut daemon: Daemon, app: &Client, shell: &Client) {
    // Stored: a restarted service has the same events.
    daemon.crash();
    let mut again = start(world);
    front(world, Some("amal"));
    assert!(wait_until(WAIT, || call(shell, &Day { date: TODAY })
        .unwrap()
        .events
        .len()
        == 1));
    assert_eq!(again.terminate(), Some(0));
    // A damaged book is reported, never overwritten.
    let file = world.dir.join("books/amal.cal");
    std::fs::write(&file, b"ULTARSTATE1\nbroken").unwrap();
    let mut third = start(world);
    front(world, Some("amal"));
    assert!(wait_until(WAIT, || call(app, &add("x", 1))
        .unwrap()
        .outcome
        == CalendarOutcome::Unavailable));
    assert_eq!(std::fs::read(&file).unwrap(), b"ULTARSTATE1\nbroken");
    assert_eq!(third.terminate(), Some(0));
}

fn switch_people_and_refusals(world: &World, app: &Client, shell: &Client) {
    // Another person sees an empty calendar; amal's comes back with her.
    front(world, Some("badr"));
    assert!(wait_until(WAIT, || call(shell, &Day { date: TODAY })
        .unwrap()
        .events
        .is_empty()));
    front(world, Some("amal"));
    assert!(wait_until(WAIT, || call(shell, &Day { date: TODAY })
        .unwrap()
        .events
        .len()
        == 2));
    assert_eq!(
        call(app, &Remove { id: 1 }).unwrap().outcome,
        CalendarOutcome::Done(1)
    );
    assert_eq!(
        call(app, &Remove { id: 1 }).unwrap().outcome,
        CalendarOutcome::Unknown
    );
    // The shell may only read; other apps nothing.
    assert!(matches!(
        shell.call(&add("x", 0), WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    let other = world.client("app.other");
    assert!(matches!(
        other.call(&Day { date: TODAY }, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
}
