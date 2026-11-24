//! The real `ultar-audiod` on a machine tree without sound cards: it says so, and every
//! change answers "no device"; apps without a rule are refused by the broker.

use std::time::Duration;
use ultar_bus::messages::audio::{
    AudioOutcome, AudioState, Outputs, SetMute, SetOutput, SetVolume,
};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.audiod"
register = ["audio"]
publish = ["audio.state"]

[[rule]]
identity = "system.shell"
call = ["audio.set_volume", "audio.set_mute", "audio.outputs", "audio.set_output"]
subscribe = ["audio.state"]

[[rule]]
identity = "app.player"
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

#[test]
fn without_a_sound_card_the_service_says_no_device() {
    let world = World::new(
        "audio",
        POLICY,
        &["system.audiod", "system.shell", "app.player"],
    );
    let state = world.subdir("state");
    let root = world.subdir("root");
    let mut daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-audiod"),
        "system.audiod",
        &[
            "--state",
            state.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
        ],
    );
    let shell = world.client("system.shell");
    let topic = shell.subscribe::<AudioState>().unwrap();
    let s = topic.recv_timeout(WAIT).unwrap();
    assert!(!s.present && s.output_id.is_none());
    assert_eq!(
        call(&shell, &SetVolume { percent: 50 }).unwrap().outcome,
        AudioOutcome::NoDevice
    );
    assert_eq!(
        call(&shell, &SetMute { on: true }).unwrap().outcome,
        AudioOutcome::NoDevice
    );
    assert_eq!(
        call(&shell, &SetOutput { id: "card0".into() })
            .unwrap()
            .outcome,
        AudioOutcome::NoDevice
    );
    assert!(call(&shell, &Outputs {}).unwrap().outputs.is_empty());
    let app = world.client("app.player");
    assert!(matches!(
        app.call(&SetVolume { percent: 100 }, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    assert_eq!(daemon.terminate(), Some(0));
}
