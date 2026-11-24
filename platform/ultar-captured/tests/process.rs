//! The real `ultar-captured` with the real compositor (headless, in this process):
//! a screenshot of the person in front lands in their Pictures folder as a PNG.

use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::account_local::LocalUsers;
use ultar_bus::messages::capture::{Screenshot, ShotOutcome};
use ultar_bus::messages::session::SessionState;
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_surface::proto::Rights;
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.captured"
register = ["capture"]
subscribe = ["session.state", "account.users"]

[[rule]]
identity = "system.sessiond"
publish = ["session.state"]

[[rule]]
identity = "system.shell"
call = ["capture.screenshot"]

[[rule]]
identity = "app.spy"
call = []
"#;

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

fn compositor(path: std::path::PathBuf, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut rights = RightsTable::default();
        rights.grant(ultar_service::effective_uid(), Rights::CAPTURE);
        let config = Config {
            rights,
            cursor: false,
            ..Config::default()
        };
        let mut comp = Compositor::new(
            config,
            Box::new(Headless::new(&[(64, 48)])),
            Box::new(MonotonicClock::default()),
        );
        comp.set_listener(UnixListener::bind(&path).unwrap())
            .unwrap();
        while !stop.load(Ordering::SeqCst) {
            let _ = comp.dispatch(Some(Duration::from_millis(10)));
        }
    });
}

#[test]
#[rustfmt::skip]
fn the_screenshot_lands_in_the_pictures_of_the_person_in_front() {
    const IDS: [&str; 4] = ["system.captured", "system.sessiond", "system.shell", "app.spy"];
    let world = World::new("cap", POLICY, &IDS);
    let homes = world.subdir("home");
    std::fs::create_dir_all(homes.join("amal")).unwrap();
    let socket = world.dir.join("compositor.sock");
    let stop = Arc::new(AtomicBool::new(false));
    let mut daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-captured"),
        "system.captured",
        &[
            "--compositor", socket.to_str().unwrap(),
            "--homes", homes.to_str().unwrap(),
            "--no-chown", "yes",
        ],
    );
    let shell = world.client("system.shell");
    assert_eq!(
        call(&shell, &Screenshot { output: None }).unwrap().outcome,
        ShotOutcome::NoUser
    );
    let sessions = world.client("system.sessiond");
    sessions
        .publish(&SessionState {
            user: Some("amal".into()),
            signed_in: vec!["amal".into()],
        })
        .unwrap();
    assert!(wait_until(WAIT, || call(
        &shell,
        &Screenshot { output: None }
    )
    .unwrap()
    .outcome
        == ShotOutcome::NoDisplay));
    compositor(socket.clone(), stop.clone());
    check_saved_png(&shell);
    assert_eq!(
        call(&shell, &Screenshot { output: Some(999) })
            .unwrap()
            .outcome,
        ShotOutcome::NoDisplay
    );
    let spy = world.client("app.spy");
    assert!(matches!(
        spy.call(&Screenshot { output: None }, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    let _ = LocalUsers {
        users: vec![],
        credentials_available: true,
    };
    assert_eq!(daemon.terminate(), Some(0));
    stop.store(true, Ordering::SeqCst);
}

/// Waits for a saved screenshot and checks it is the compositor's 64×48 output.
fn check_saved_png(shell: &Client) {
    let mut path = None;
    assert!(wait_until(WAIT, || {
        match call(shell, &Screenshot { output: None }).unwrap().outcome {
            ShotOutcome::Saved(p) => {
                path = Some(p);
                true
            }
            _ => false,
        }
    }));
    let bytes = std::fs::read(path.unwrap()).unwrap();
    let mut reader = png::Decoder::new(bytes.as_slice()).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (64, 48));
}
