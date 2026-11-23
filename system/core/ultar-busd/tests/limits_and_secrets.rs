//! Review round 3: the token table from an inherited descriptor (R3-K1,
//! runtime side) and the start-time check of the connection bounds
//! (R3-B1).

mod common;

use common::{busd, temp_dir, terminate, BUSD, WAIT};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use ultar_bus::{BusError, Client, ClientOptions};
use ultar_service::exit;

#[test]
fn the_token_table_can_come_from_an_inherited_descriptor() {
    let dir = temp_dir("token-fd");
    let socket = dir.join("bus.sock");
    let tokens = dir.join("tokens");
    std::fs::write(&tokens, "system.devd = \"0123456789abcdef-devd\"\n").unwrap();
    // A file mode the checks would refuse: a descriptor is not judged by it.
    std::fs::set_permissions(&tokens, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("exec \"$0\" \"$@\" 3<'{}'", tokens.display()))
        .arg(BUSD)
        .args(["--lang", "en", "--listen-token"])
        .arg(&socket)
        .args(["--token-fd", "3", "--allow-all"])
        .env_remove("ULTAR_BUS_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if ultar_service::effective_uid() == 0 {
        command.arg("--allow-root");
    }
    let child = command.spawn().unwrap();
    let with = |token: &str| {
        let options = ClientOptions {
            connect_retry_for: WAIT,
            token: Some(token.to_owned()),
            ..ClientOptions::named("t")
        };
        Client::connect(&socket, options)
    };
    assert_eq!(
        with("0123456789abcdef-devd").unwrap().identity(),
        "system.devd"
    );
    assert!(matches!(
        with("0123456789abcdef-devX"),
        Err(BusError::AuthFailed)
    ));
    assert_eq!(terminate(child).0, Some(0));
}

#[test]
fn a_descriptor_that_was_not_passed_stops_the_start() {
    let dir = temp_dir("token-fd-missing");
    let output = busd()
        .arg("--listen-token")
        .arg(dir.join("bus.sock"))
        .args(["--token-fd", "7", "--allow-all"])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
    assert!(!dir.join("bus.sock").exists());
}

#[test]
fn bounds_beyond_the_broker_limits_stop_the_start() {
    let dir = temp_dir("too-many");
    let output = busd()
        .arg("--listen")
        .arg(format!("{}=app.a", dir.join("a.sock").display()))
        .args(["--max-connections", "600"])
        .arg("--listen")
        .arg(format!("{}=app.b", dir.join("b.sock").display()))
        .args(["--max-connections", "600", "--allow-all"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(exit::FAILURE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lower --max-connections: descriptors"),
        "{stderr}"
    );
    assert!(!dir.join("a.sock").exists());
}
