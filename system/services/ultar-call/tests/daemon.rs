//! The real `ultar-calld` binary with the test-double modem in its process, over a real bus:
//! state from the modem, a call dialled and ended, the history written 0600 and kept.

use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::call::{CallState, Cellular, Dial, Hangup, Log, Phase, Way};
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};

const WAIT: Duration = Duration::from_secs(20);

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait(app: &Client, f: impl Fn(&CallState) -> bool) -> CallState {
    let sub = app.subscribe::<CallState>().unwrap();
    loop {
        let st = sub.recv_timeout(WAIT).unwrap();
        if f(&st) {
            return st;
        }
    }
}

fn start(sock: &std::path::Path, log: &std::path::Path) -> Daemon {
    let child = Command::new(env!("CARGO_BIN_EXE_ultar-calld"))
        .args([
            "--bus",
            sock.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
            "--fake-modem",
            "--lang",
            "en",
        ])
        .spawn()
        .unwrap();
    Daemon(child)
}

#[test]
fn daemon_with_the_fake_modem() {
    let dir = std::env::temp_dir().join(format!("calld-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("bus.sock");
    let log = dir.join("calls.log");
    let auth = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&sock, auth)
        .start()
        .unwrap();
    let daemon = start(&sock, &log);
    let app = Client::connect(&sock, ClientOptions::named("phone")).unwrap();
    wait(&app, |s| s.lines.cellular == Cellular::Ready);
    let id = app
        .call(
            &Dial {
                way: Way::Cellular(1),
                number: "0933".into(),
                video: false,
            },
            WAIT,
        )
        .unwrap()
        .id;
    wait(&app, |s| {
        s.calls
            .iter()
            .any(|c| c.id == id && c.phase == Phase::Alerting)
    });
    app.call(&Hangup { id }, WAIT).unwrap();
    wait(&app, |s| s.calls.is_empty());
    assert_eq!(
        app.call(&Log { missed_only: false }, WAIT)
            .unwrap()
            .entries
            .len(),
        1
    );
    let mode = std::fs::metadata(&log).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    drop(daemon);
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.starts_with("ULTAR-CALLLOG 1") && text.contains("0933"),
        "history on disk"
    );
    // The broker wakes its acceptor through the socket file: it goes before the directory.
    drop(app);
    drop(broker);
    std::fs::remove_dir_all(&dir).unwrap();
}
