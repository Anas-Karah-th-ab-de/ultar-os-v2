//! The real `ultar-volumed` on a prepared kernel tree: a stick is seen for the person in
//! front; mounting a regular file is refused by the kernel, so nothing is shown (no fake
//! drive); ejecting unknown drives answers `Unknown`; apps are refused.

use std::time::Duration;
use ultar_bus::messages::account_local::LocalUsers;
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::volumes::{Eject, EjectOutcome, Usage, VolumesState, INTERNAL};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.volumed"
register = ["volumes"]
publish = ["volumes.state"]
subscribe = ["session.state", "account.users"]

[[rule]]
identity = "system.sessiond"
publish = ["session.state"]

[[rule]]
identity = "system.shell"
call = ["volumes.eject", "volumes.usage"]
subscribe = ["volumes.state"]

[[rule]]
identity = "app.any"
call = []
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

/// How full the device is: its own storage (nothing is mounted), as the kernel says.
fn usage_is_measured(shell: &Client, root: &std::path::Path) {
    let usage = call(shell, &Usage {}).unwrap();
    assert_eq!(usage.places.len(), 1, "{usage:?}");
    let own = &usage.places[0];
    assert_eq!(own.id, INTERNAL);
    let measured = ultar_volumed::space_of(root).expect("statvfs");
    assert_eq!(own.total_bytes, measured.0);
    assert!(own.free_bytes <= own.total_bytes && own.total_bytes > 0);
}

fn an_app_without_the_right_gets_nothing(app: &Client) {
    assert!(matches!(
        app.call(&Eject { id: "sdz1".into() }, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    assert!(matches!(
        app.call(&Usage {}, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
}

#[test]
fn nothing_is_shown_that_is_not_mounted() {
    let world = World::new(
        "vol",
        POLICY,
        &[
            "system.volumed",
            "system.sessiond",
            "system.shell",
            "app.any",
        ],
    );
    let (root, media) = (world.subdir("root"), world.subdir("media"));
    let mut fat = vec![0u8; 2048];
    fat[71..76].copy_from_slice(b"STICK");
    fat[82..87].copy_from_slice(b"FAT32");
    fat[510] = 0x55;
    fat[511] = 0xAA;
    std::fs::create_dir_all(root.join("sys/block/sdz/sdz1")).unwrap();
    std::fs::write(root.join("sys/block/sdz/removable"), "1").unwrap();
    std::fs::write(root.join("sys/block/sdz/sdz1/partition"), "1").unwrap();
    std::fs::create_dir_all(root.join("dev")).unwrap();
    std::fs::write(root.join("dev/sdz1"), &fat).unwrap();
    let mut daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-volumed"),
        "system.volumed",
        &[
            "--root",
            root.to_str().unwrap(),
            "--media",
            media.to_str().unwrap(),
        ],
    );
    let shell = world.client("system.shell");
    let state = shell.subscribe::<VolumesState>().unwrap();
    assert_eq!(state.recv_timeout(WAIT).unwrap(), VolumesState::default());
    world
        .client("system.sessiond")
        .publish(&SessionState {
            user: Some("amal".into()),
            signed_in: vec!["amal".into()],
        })
        .unwrap();
    let _ = LocalUsers {
        users: vec![],
        credentials_available: true,
    };
    // The "device" is a regular file: the kernel refuses it, the folder is removed again.
    std::thread::sleep(Duration::from_secs(3));
    assert!(call(&shell, &Eject { id: "sdz1".into() }).unwrap().outcome == EjectOutcome::Unknown);
    assert!(std::fs::read_dir(&media)
        .unwrap()
        .flatten()
        .all(|d| std::fs::read_dir(d.path()).unwrap().next().is_none()));
    usage_is_measured(&shell, &root);
    an_app_without_the_right_gets_nothing(&world.client("app.any"));
    assert_eq!(daemon.terminate(), Some(0));
}
