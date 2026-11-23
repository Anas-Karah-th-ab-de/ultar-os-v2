//! End to end: the real `ultar-settingsd` binary on a temporary bus socket,
//! driven by the real `usettings` binary.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::Shutdown;
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_profile::{Capabilities, Capability, CapabilitySet, DeviceProfile, Edition, FormFactor};
use ultar_settingsd::testdir::TestDir;

/// `--allow-root` when the tests run as root, nothing otherwise.
fn root_flag() -> &'static [&'static str] {
    if ultar_service::effective_uid() == 0 {
        &["--allow-root"]
    } else {
        &[]
    }
}

const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-settingsd");
const CLI: &str = env!("CARGO_BIN_EXE_usettings");
const WAIT: Duration = Duration::from_secs(30);

struct World {
    // Dropped first: the broker needs its socket file to shut down.
    _broker: Broker,
    socket: PathBuf,
    dir: TestDir,
}

struct Daemon {
    child: Child,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl World {
    fn new(tag: &str) -> Self {
        let dir = TestDir::new(tag);
        let socket = dir.path().join("bus.sock");
        let peer = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
        let broker = Broker::builder(Policy::allow_all())
            .listen(&socket, peer)
            .start()
            .unwrap();
        let profile = DeviceProfile {
            device_id: "Test Laptop | test".into(),
            form_factor: FormFactor::Laptop,
            edition: Edition::Desktop,
            capabilities: Capabilities {
                flags: CapabilitySet::of(&[Capability::Battery, Capability::Keyboard]),
                display_count: 1,
                ..Capabilities::default()
            },
        };
        std::fs::write(dir.path().join("profile.toml"), profile.to_toml().unwrap()).unwrap();
        Self {
            _broker: broker,
            socket,
            dir,
        }
    }

    fn store(&self) -> PathBuf {
        self.dir.path().join("store")
    }

    /// Starts the daemon and waits for its "ready" line.
    fn start_daemon(&self) -> Daemon {
        let mut child = Command::new(DAEMON)
            .arg("--store")
            .arg(self.store())
            .arg("--socket")
            .arg(&self.socket)
            .arg("--profile")
            .arg(self.dir.path().join("profile.toml"))
            .args([
                "--allow-all",
                "--user",
                "anas",
                "--lang",
                "en",
                "--write-delay-ms",
                "20",
            ])
            // Root has to be declared (the tests may run as root).
            .args(root_flag())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "Settings service ready");
        Daemon { child }
    }

    fn cli_command(&self, lang: &str, words: &[&str]) -> Command {
        let mut command = Command::new(CLI);
        command
            .arg("--socket")
            .arg(&self.socket)
            .args(["--lang", lang])
            .args(words)
            .env_remove("ULTAR_BUS_SOCKET");
        command
    }

    fn cli(&self, lang: &str, words: &[&str]) -> Output {
        self.cli_command(lang, words).output().unwrap()
    }

    /// Runs the command and returns its standard output; it must succeed.
    fn ok(&self, lang: &str, words: &[&str]) -> String {
        let output = self.cli(lang, words);
        let text = String::from_utf8(output.stdout).unwrap();
        let errors = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "{words:?} failed: {text} {errors}");
        text
    }

    /// Runs the command, which must fail with `status`; returns its
    /// standard error.
    fn fails(&self, lang: &str, words: &[&str], status: i32) -> String {
        let output = self.cli(lang, words);
        assert_eq!(output.status.code(), Some(status), "{words:?}");
        String::from_utf8(output.stderr).unwrap()
    }

    fn shutdown(&self, mut daemon: Daemon) {
        let client = Client::connect(&self.socket, ClientOptions::named("test")).unwrap();
        client.call(&Shutdown, WAIT).unwrap();
        assert!(daemon.child.wait().unwrap().success());
    }
}

fn line(reader: &mut BufReader<ChildStdout>) -> String {
    let mut text = String::new();
    reader.read_line(&mut text).unwrap();
    text.trim_end().to_owned()
}

fn file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn get_and_set_in_three_languages() {
    let world = World::new("e2e-basic");
    let daemon = world.start_daemon();

    assert_eq!(
        world.ok("en", &["get", "appearance.theme"]),
        "appearance.theme = dark\n"
    );
    assert_eq!(
        world.ok("en", &["set", "appearance.theme", "light"]),
        "appearance.theme set to light\n"
    );
    assert_eq!(
        world.ok("de", &["set", "appearance.theme", "light"]),
        "appearance.theme ist bereits light\n"
    );
    assert_eq!(
        world.ok("ar", &["set", "appearance.font_size", "120"]),
        "تم ضبط appearance.font_size على 120\n"
    );
    assert_eq!(
        world.ok("ar", &["get", "appearance.theme"]),
        "appearance.theme = light\n"
    );

    world.shutdown(daemon);
}

#[test]
fn list_shows_what_the_device_has_and_values_are_written_at_shutdown() {
    let world = World::new("e2e-list");
    let daemon = world.start_daemon();
    world.ok("en", &["set", "appearance.theme", "light"]);
    world.ok("en", &["set", "appearance.font_size", "120"]);
    let power = world.ok("en", &["list", "power"]);
    assert!(power.starts_with("[Power]\n"), "{power}");
    assert!(
        power.contains("power.mode = balanced    Power mode\n"),
        "{power}"
    );
    // The profile has a battery.
    assert!(
        power.contains("power.charge_limit.percent = 80    Charge limit level\n"),
        "{power}"
    );
    let arabic = world.ok("ar", &["list", "region"]);
    assert!(arabic.starts_with("[المنطقة واللغة]\n"), "{arabic}");
    assert!(
        arabic.contains("region.language = ar    لغة النظام\n"),
        "{arabic}"
    );
    let everything = world.ok("de", &["list"]);
    assert!(everything.contains("[Energie]"));
    // No camera in the profile: its master switch is not listed.
    assert!(!everything.contains("privacy.camera"));
    assert!(everything.contains("privacy.network"));

    assert_eq!(
        world.ok("en", &["reset", "appearance.theme"]),
        "appearance.theme is back at its default\n"
    );
    assert_eq!(
        world.ok("en", &["reset", "appearance.theme"]),
        "appearance.theme already has its default\n"
    );
    world.shutdown(daemon);
    // Everything was written before the daemon ended.
    let user = file(&world.store().join("user-anas.toml"));
    assert!(user.contains("\"appearance.font_size\" = 120"), "{user}");
    assert!(!user.contains("appearance.theme"), "{user}");
}

#[test]
fn errors_are_reported_in_the_users_language_with_exit_codes() {
    let world = World::new("e2e-errors");
    let daemon = world.start_daemon();
    assert_eq!(
        world.fails("en", &["get", "no.such.key"], 1),
        "There is no setting with this name\n"
    );
    assert_eq!(
        world.fails("en", &["set", "appearance.theme", "pink"], 1),
        "The value is not accepted for this setting\n"
    );
    assert_eq!(
        world.fails("ar", &["set", "appearance.font_size", "5000"], 1),
        "القيمة خارج المجال المسموح\n"
    );
    assert_eq!(
        world.fails("de", &["set", "privacy.camera", "off"], 1),
        "Diese Einstellung gibt es auf diesem Gerät nicht\n"
    );
    assert_eq!(
        world.fails("en", &["confirm", "privacy.network"], 1),
        "No change is waiting to be confirmed\n"
    );
    let usage = world.fails("en", &["frobnicate"], 2);
    assert!(
        usage.starts_with("Invalid command\nUsage: usettings"),
        "{usage}"
    );
    let option = world.fails("en", &["--frob"], 2);
    assert!(option.starts_with("Unknown option: --frob\n"), "{option}");
    assert!(world.ok("en", &["--help"]).starts_with("Usage: usettings"));
    world.shutdown(daemon);
    // Without a service the command says so instead of hanging.
    assert_eq!(
        world.fails("en", &["get", "appearance.theme"], 1),
        "The settings service is not running\n"
    );
    // Without a socket it cannot even try.
    let output = Command::new(CLI)
        .args(["--lang", "en", "get", "appearance.theme"])
        .env_remove("ULTAR_BUS_SOCKET")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn watch_prints_changes_as_they_happen() {
    let world = World::new("e2e-watch");
    let daemon = world.start_daemon();
    let mut watch = world
        .cli_command("en", &["watch", "--count", "2"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(watch.stdout.take().unwrap());
    // The command says when it listens; only then are the changes made.
    assert_eq!(line(&mut lines), "Watching for setting changes…");
    world.ok("en", &["set", "appearance.accent_color", "blue"]);
    world.ok("en", &["set", "power.mode", "saver"]);
    assert_eq!(line(&mut lines), "appearance.accent_color = blue");
    assert_eq!(line(&mut lines), "power.mode = saver");
    assert!(watch.wait().unwrap().success());
    world.shutdown(daemon);
}

#[test]
fn confirm_revert_and_restart_during_a_pending_change() {
    let world = World::new("e2e-confirm");
    let daemon = world.start_daemon();
    let key = "privacy.network";

    // Kept.
    let set = world.ok("en", &["set", key, "off"]);
    assert_eq!(
        set,
        "privacy.network set to false\n\
         The change is taken back in 15 seconds unless you confirm it: usettings confirm privacy.network\n"
    );
    let listed = world.ok("en", &["list", "privacy"]);
    assert!(
        listed.contains("privacy.network = false    Network (waiting for confirmation)\n"),
        "{listed}"
    );
    assert_eq!(world.ok("en", &["confirm", key]), "Changes kept: 1\n");
    assert_eq!(world.ok("en", &["get", key]), "privacy.network = false\n");

    // Taken back on request.
    world.ok("en", &["set", key, "on"]);
    assert_eq!(
        world.ok("ar", &["revert"]),
        "عدد التغييرات التي تُراجِع عنها: 1\n"
    );
    assert_eq!(world.ok("en", &["get", key]), "privacy.network = false\n");
    world.ok("en", &["set", key, "on"]);
    world.ok("en", &["confirm"]);
    world.shutdown(daemon);

    // The daemon is killed while a change waits for "keep?".
    let mut daemon = world.start_daemon();
    assert_eq!(world.ok("en", &["get", key]), "privacy.network = true\n");
    world.ok("en", &["set", key, "off"]);
    assert_eq!(world.ok("en", &["get", key]), "privacy.network = false\n");
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    drop(daemon);

    // On the next start the unconfirmed change is gone.
    let daemon = world.start_daemon();
    assert_eq!(world.ok("en", &["get", key]), "privacy.network = true\n");
    assert_eq!(
        world.fails("en", &["confirm", key], 1),
        "No change is waiting to be confirmed\n"
    );
    world.shutdown(daemon);
}

#[test]
fn daemon_reports_a_damaged_file_and_refuses_bad_command_lines() {
    let world = World::new("e2e-daemon");
    std::fs::create_dir_all(world.store()).unwrap();
    std::fs::write(world.store().join("user-anas.toml"), "appearance = broken").unwrap();
    let daemon = world.start_daemon();
    assert_eq!(
        world.ok("en", &["get", "appearance.theme"]),
        "appearance.theme = dark\n"
    );
    assert_eq!(
        file(&world.store().join("quarantine/user-anas.toml.0")),
        "appearance = broken"
    );
    world.shutdown(daemon);

    let run = |args: &[&str]| {
        Command::new(DAEMON)
            .args(args)
            .args(root_flag())
            .args(["--lang", "en"])
            .env_remove("ULTAR_BUS_SOCKET")
            .output()
            .unwrap()
    };
    let no_policy = run(&["--store", "/tmp/x", "--socket", "/tmp/y"]);
    assert_eq!(no_policy.status.code(), Some(2));
    let text = String::from_utf8(no_policy.stderr).unwrap();
    assert!(
        text.starts_with("Either --policy FILE or --allow-all is required\nUsage: ultar-settingsd"),
        "{text}"
    );
    assert_eq!(run(&["--frob"]).status.code(), Some(2));
    let help = run(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)
        .unwrap()
        .starts_with("Usage: ultar-settingsd"));
    let missing = world.dir.path().join("missing.toml");
    let bad_policy = run(&[
        "--store",
        "/tmp/x",
        "--socket",
        "/tmp/y",
        "--lang",
        "en",
        "--policy",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(bad_policy.status.code(), Some(1));
    assert!(String::from_utf8(bad_policy.stderr)
        .unwrap()
        .starts_with("The policy file is not valid"));
}
