//! Secrets from inherited descriptors, the policy read again on `SIGHUP`,
//! and a stop while the bus is still awaited (review round 3: R3-K1
//! runtime side, R3-P1, R3-S1).

mod common;

use common::world::{ready, send_signal, stop, World, LOGD, POLICY, ULOG};
use common::WAIT;
use std::process::Command;
use std::time::Instant;
use ultar_bus::messages::log::Query;

/// The chain key and the bus token arrive on descriptors 3 and 4 (as a
/// service manager passes them on a kernel whose files cannot be
/// checked); the log joins the bus with the token and is keyed.
#[test]
fn chain_key_and_bus_token_are_read_from_inherited_descriptors() {
    let world = World::new("secret-fds");
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(format!(
        "exec \"$0\" \"$@\" 3<'{}' 4<'{}'",
        world.path("chain.key").display(),
        world.path("logd.token").display()
    ));
    shell.arg(LOGD);
    let secrets = [
        "--chain-key-fd".as_ref(),
        "3".as_ref(),
        "--bus-token-fd".as_ref(),
        "4".as_ref(),
    ];
    let child = ready(world.logd_with(shell, &secrets, &[]));
    let admin = world.client("admin.support");
    world.wait_for_key(&admin, "log.bus_joined");
    assert_eq!(stop(child).0, Some(0));
    // Verified with the key file: the descriptor carried the same key.
    world.verify(true);
    // The tool takes the key from a descriptor as well.
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "exec \"$0\" --lang en verify \"$1\" --key-fd 3 3<'{}'",
            world.path("chain.key").display()
        ))
        .arg(ULOG)
        .arg(world.path("log"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        !text.contains("unkeyed") && !text.contains("without a key"),
        "{text}"
    );
}

/// `SIGHUP` reads the policy again: a revoked `log.read_all` ends without
/// a restart.
#[test]
fn a_reload_revokes_reading_everything() {
    let world = World::new("reload");
    let child = world.start(&[]);
    let admin = world.client("admin.support");
    world.wait_for_key(&admin, "log.bus_joined");
    let revoked = POLICY.replace(r#", "log.read_all"]"#, "]");
    assert_ne!(revoked, POLICY);
    std::fs::write(world.path("policy.toml"), revoked).unwrap();
    send_signal(child.id(), "HUP");
    let deadline = Instant::now() + WAIT;
    loop {
        let reply = world.query(&admin, &Query::default());
        if reply.records.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "still reads everything");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(stop(child).0, Some(0));
}

/// A stop while the bus is not there yet ends the service at once with
/// exit 0, not after the stop deadline.
#[test]
fn a_stop_while_the_bus_is_awaited_exits_with_zero() {
    let world = World::new("no-bus");
    let child = world.start(&["--bus", "/nonexistent/ultar-bus.sock"]);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let started = Instant::now();
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}
