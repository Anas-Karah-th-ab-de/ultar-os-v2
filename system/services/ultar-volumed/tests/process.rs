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
subscribe = ["session.state", "account.users", "settings.changed", "notify.invoked"]
call = ["settings.get", "notify.post", "notify.dismiss", "notify.take_invoked"]

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

const SETTINGS_POLICY: &str = r#"
[[rule]]
identity = "system.volumed"
register = ["volumes"]
publish = ["volumes.state"]
subscribe = ["session.state", "account.users", "settings.changed", "notify.invoked"]
call = ["settings.get", "notify.post", "notify.dismiss", "notify.take_invoked"]

[[rule]]
identity = "system.stand_in"
register = ["settings", "notify"]
publish = ["session.state", "account.users", "settings.changed", "notify.invoked"]
"#;

/// The Storage settings the stand-in holds (read at start, changed at runtime).
type Values = std::sync::Arc<
    std::sync::Mutex<
        std::collections::BTreeMap<String, ultar_bus::messages::settings::SettingValue>,
    >,
>;
/// The notifications the stand-in received.
type Posts = std::sync::Arc<std::sync::Mutex<Vec<ultar_bus::messages::notify::Post>>>;

fn settings_map() -> Values {
    use ultar_bus::messages::settings::SettingValue;
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        "storage.low_space_percent".into(),
        SettingValue::Integer(10),
    );
    m.insert("storage.automount".into(), SettingValue::Flag(false));
    std::sync::Arc::new(std::sync::Mutex::new(m))
}

/// The settings and notification stand-ins: settings answer from `values`, notifications
/// are kept in `posts` and answered with a rising id.
fn serve_stand_ins(world: &World, values: &Values, posts: &Posts) {
    use ultar_bus::messages::notify::{
        NoteOutcome, NoteReply, Post, PostReply, TakeInvoked, TakeInvokedReply,
    };
    use ultar_bus::messages::settings::{Get, GetReply};
    let v = values.clone();
    world.serve("system.stand_in", "settings", move |request| {
        let value = request
            .decode::<Get>()
            .ok()
            .and_then(|g| v.lock().unwrap().get(&g.key).cloned());
        let _ = request.reply(&GetReply { value });
    });
    let p = posts.clone();
    world.serve("system.stand_in", "notify", move |request| {
        if let Ok(post) = request.decode::<Post>() {
            let id = {
                let mut seen = p.lock().unwrap();
                seen.push(post);
                seen.len() as u64
            };
            let _ = request.reply(&PostReply { id });
        } else if request.decode::<TakeInvoked>().is_ok() {
            let _ = request.reply(&TakeInvokedReply {
                invoked: Vec::new(),
            });
        } else {
            let _ = request.reply(&NoteReply {
                outcome: NoteOutcome::Done,
            });
        }
    });
}

/// The person in front: `amal` with the kernel id `me`.
fn sign_in(stand_in: &Client, me: u32) {
    use ultar_bus::messages::account_local::{LocalUser, PictureInfo, UserKind};
    stand_in
        .publish(&SessionState {
            user: Some("amal".into()),
            signed_in: vec!["amal".into()],
        })
        .unwrap();
    stand_in
        .publish(&LocalUsers {
            users: vec![LocalUser {
                name: "amal".into(),
                uid: me,
                display_name: "Amal".into(),
                kind: UserKind::Standard,
                picture: PictureInfo::Initials,
                auto_login: false,
                current: true,
            }],
            credentials_available: true,
        })
        .unwrap();
}

/// A removable "drive" `sdz1` (a regular file the kernel refuses to mount).
fn make_drive(root: &std::path::Path) {
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
}

/// With automount off the drive is offered with one "Mount" button.
fn check_offer(posts: &Posts) {
    let seen = || {
        posts
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.title.key == "volumed.drive.title")
    };
    assert!(
        wait_until(WAIT, seen),
        "no drive offer: {:?}",
        posts.lock().unwrap()
    );
    let offer = posts
        .lock()
        .unwrap()
        .iter()
        .find(|p| p.title.key == "volumed.drive.title")
        .cloned()
        .unwrap();
    assert_eq!(offer.actions.len(), 1);
    assert_eq!(offer.actions[0].id, "mount");
}

/// The real daemon follows `settings.changed`: with automount off a drive is offered with
/// a "Mount" button instead of mounted. (The downloads clean-up is the launcher's now –
/// ultar-launcher `tests/clean.rs` checks it.)
#[test]
fn settings_changed_drive_the_offer() {
    let world = World::new(
        "volset",
        SETTINGS_POLICY,
        &["system.volumed", "system.stand_in"],
    );
    let (root, media) = (world.subdir("root"), world.subdir("media"));
    let me = rustix::process::getuid().as_raw();
    let (values, posts) = (
        settings_map(),
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    serve_stand_ins(&world, &values, &posts);
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
    let stand_in = world.client("system.stand_in");
    sign_in(&stand_in, me);
    // Automount is off from the start: once the settings are read, a new drive is offered.
    std::thread::sleep(Duration::from_secs(2));
    make_drive(&root);
    check_offer(&posts);
    assert_eq!(daemon.terminate(), Some(0));
}
