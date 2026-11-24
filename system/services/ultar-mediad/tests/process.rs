//! The real `ultar-mediad`: a player holds the session, the lock screen's buttons reach
//! it, an app cannot press buttons.

use std::time::Duration;
use ultar_bus::messages::media::{
    Button, Command, MediaOutcome, MediaState, Next, PlayPause, Release, Update,
};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.mediad"
register = ["media"]
publish = ["media.state", "media.command"]

[[rule]]
identity = "system.lock"
call = ["media.play_pause", "media.next", "media.previous", "media.system"]
subscribe = ["media.state"]

[[rule]]
identity = "app.*"
call = ["media.update", "media.release", "media.play_pause"]
subscribe = ["media.command"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

#[test]
fn the_lock_screen_controls_the_player() {
    let world = World::new(
        "media",
        POLICY,
        &["system.mediad", "system.lock", "app.music", "app.other"],
    );
    let policy = world.policy_file();
    let mut daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-mediad"),
        "system.mediad",
        &["--policy", policy.to_str().unwrap()],
    );
    let lock = world.client("system.lock");
    let music = world.client("app.music");
    let other = world.client("app.other");
    let state = lock.subscribe::<MediaState>().unwrap();
    let commands = music.subscribe::<Command>().unwrap();
    assert_eq!(
        call(&lock, &PlayPause {}).unwrap().outcome,
        MediaOutcome::NoPlayer
    );
    assert_eq!(state.recv_timeout(WAIT).unwrap(), MediaState::default());
    let up = Update {
        title: "Song".into(),
        artist: "Band".into(),
        playing: true,
        can_next: true,
        can_previous: false,
    };
    assert_eq!(call(&music, &up).unwrap().outcome, MediaOutcome::Done);
    assert!(wait_until(WAIT, || state
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(|s| s
            .now
            .is_some_and(|n| n.player == "app.music"))));
    assert_eq!(call(&lock, &Next {}).unwrap().outcome, MediaOutcome::Done);
    let c = commands.recv_timeout(WAIT).unwrap();
    assert_eq!((c.player.as_str(), c.button), ("app.music", Button::Next));
    // An app may call play_pause (broker), the service does not let it press.
    assert_eq!(
        call(&other, &PlayPause {}).unwrap().outcome,
        MediaOutcome::Unsupported
    );
    assert_eq!(
        call(&other, &Release {}).unwrap().outcome,
        MediaOutcome::Unsupported
    );
    assert_eq!(
        call(&music, &Release {}).unwrap().outcome,
        MediaOutcome::Done
    );
    assert!(wait_until(WAIT, || state
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(|s| s.now.is_none())));
    assert_eq!(daemon.terminate(), Some(0));
}
