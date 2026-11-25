//! The downloads clean-up on the bus: the launcher reads `storage.clean_downloads` at
//! start, follows `settings.changed`, knows the person in front (`session.state`) and
//! their id (`account.users`), and removes their old regular files in the downloads area
//! – the fresh ones stay. The launcher owns the areas; the volume service no longer
//! reaches them.

mod common;

use common::World;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::account_local::{LocalUser, LocalUsers, PictureInfo, UserKind};
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::settings::{Changed, Get, GetReply, SettingValue};
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_launcher::follow;
use ultar_service::{Lifecycle, StopCause};

const POLICY: &str = r#"
[[rule]]
identity = "system.launcher"
register = ["launcher"]
call = ["settings.get"]
subscribe = ["settings.changed", "session.state", "account.users"]

[[rule]]
identity = "system.stand_in"
register = ["settings"]
publish = ["settings.changed", "session.state", "account.users"]
"#;

/// Waits until `f` holds (the launcher's follow loop polls every 200 ms).
fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + common::WAIT;
    while !f() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn broker(world: &World) -> Broker {
    let fixed = |name: &str| Arc::new(FixedIdentity::new(Identity::new(name).unwrap()));
    Broker::builder(Policy::from_toml_str(POLICY).unwrap())
        .listen(&world.path("launcher.sock"), fixed("system.launcher"))
        .listen(&world.path("stand-in.sock"), fixed("system.stand_in"))
        .start()
        .unwrap()
}

/// The settings stand-in: the clean-up is off at start, the window is 30 days.
fn stand_in(world: &World) -> Client {
    let client = Client::connect(
        &world.path("stand-in.sock"),
        ClientOptions::named("clean-test"),
    )
    .unwrap();
    let endpoint = client.register("settings").unwrap();
    std::thread::spawn(move || {
        while let Ok(req) = endpoint.recv() {
            let _ = req.respond::<Get>(|g| {
                let value = match g.key.as_str() {
                    "storage.clean_downloads" => Some(SettingValue::Flag(false)),
                    "storage.clean_downloads_days" => Some(SettingValue::Integer(30)),
                    _ => None,
                };
                Ok(GetReply { value })
            });
        }
    });
    client
}

/// The person in front: `amal` with the kernel id of this test.
fn sign_in(stand_in: &Client) {
    let me = rustix::process::getuid().as_raw();
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

/// The downloads area with an old and a fresh file of the person; their paths.
fn downloads(world: &World) -> (PathBuf, PathBuf) {
    let dir = world.path("areas/downloads");
    std::fs::create_dir_all(&dir).unwrap();
    let (old, fresh) = (dir.join("old.bin"), dir.join("fresh.bin"));
    std::fs::write(&old, b"old").unwrap();
    std::fs::write(&fresh, b"fresh").unwrap();
    let long_ago = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000 - 40 * 86_400);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    (old, fresh)
}

#[test]
fn the_launcher_cleans_the_persons_downloads_when_switched_on() {
    let mut world = World::new("clean");
    let _broker = broker(&world);
    // The areas root is the launcher's; the downloads folder holds the person's files.
    let config = format!(
        "{}areas_root = \"{}\"\n",
        world.config_text(&["ids", "cell", "groups", "net"], None),
        world.path("areas").display()
    );
    let launcher = world.open(&config);
    let stand_in = stand_in(&world);
    let life = Lifecycle::new();
    let serving = {
        let (launcher, life) = (Arc::clone(&launcher), life.clone());
        let socket = world.path("launcher.sock");
        std::thread::spawn(move || follow::serve(socket, launcher, life))
    };
    sign_in(&stand_in);
    let (old, fresh) = downloads(&world);
    // The clean-up is off at start: nothing goes.
    std::thread::sleep(Duration::from_secs(1));
    assert!(old.exists(), "clean-up is off, nothing goes");
    // Switched on through settings.changed: the old one goes, the fresh stays.
    stand_in
        .publish(&Changed {
            key: "storage.clean_downloads".into(),
            value: SettingValue::Flag(true),
        })
        .unwrap();
    wait_until("the old download was not cleaned", || !old.exists());
    assert!(fresh.exists(), "the fresh download stays");
    life.request_stop(StopCause::Request);
    drop(_broker);
    serving.join().unwrap();
    world.close();
}
