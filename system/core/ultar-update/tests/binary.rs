//! The real programs: `uupdate-sign` builds a release from files, `ultar-updated` (a real
//! process) installs it from an HTTP server over a real bus, `uupdate` drives it.

mod common;
#[path = "common/http.rs"]
mod http;

use common::Rng;
use http::{Behaviour, TestServer};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use ultar_bus::{Broker, FixedIdentity, Identity, Policy};
use ultar_update::bootctl::{BootControl, Slot};
use ultar_update::filectl::FileBootControl;
use ultar_update::manifest::Role;

const SIGN: &str = env!("CARGO_BIN_EXE_uupdate-sign");
const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-updated");
const CLI: &str = env!("CARGO_BIN_EXE_uupdate");

fn run(program: &str, args: &[&str]) -> (i32, String) {
    let output = Command::new(program)
        .args(args)
        .args(["--lang", "en"])
        .env_remove("ULTAR_BUS_SOCKET")
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.code().unwrap_or(-1), text)
}

fn sign_ok(args: &[&str]) -> String {
    let (code, text) = run(SIGN, args);
    assert_eq!(code, 0, "uupdate-sign {args:?}: {text}");
    text
}

/// Runs a program with a command line given as one string (words split at spaces; the
/// temporary paths used here contain none).
fn run_line(program: &str, line: &str) -> (i32, String) {
    let words: Vec<&str> = line.split_whitespace().collect();
    run(program, &words)
}

fn sign_line(line: &str) -> String {
    let words: Vec<&str> = line.split_whitespace().collect();
    sign_ok(&words)
}

struct Publisher {
    dir: PathBuf,
    kernel: Vec<u8>,
    system: Vec<u8>,
}

impl Publisher {
    fn path(&self, name: &str) -> String {
        self.dir.join(name).display().to_string()
    }

    /// Copies the public release key into a new key directory, as an image build would.
    fn key_dir(&self, name: &str) -> PathBuf {
        let keys = self.dir.join(name);
        std::fs::create_dir_all(&keys).unwrap();
        std::fs::copy(
            self.dir.join("release.ed25519"),
            keys.join("release.ed25519"),
        )
        .unwrap();
        keys
    }
}

fn write_manifest(dir: &Path, version: &str, index: &str, payloads: &str) {
    let out = dir.join("manifest.txt").display().to_string();
    sign_line(&format!(
        "manifest --out {out} --version {version} --rollback-index {index} --channel stable \
         --edition desktop --arch x86_64 --devices pc-uefi,vm --min-version 0.7.0 \
         --notes-key update.notes.generic --created 1795000000 --expires 1820000000 \
         --critical {payloads}"
    ));
    let manifest = std::fs::read_to_string(dir.join("manifest.txt")).unwrap();
    assert!(manifest.contains("devices=pc-uefi,vm\n") && manifest.contains("critical=1\n"));
}

/// Builds keys and a signed bundle (`release.uup`) with the real tool.
fn publish(dir: &Path, version: &str, index: &str) -> Publisher {
    let p = |name: &str| dir.join(name).display().to_string();
    let mut rng = Rng(0xB1AA_0000 + version.len() as u64);
    let (kernel, system) = (rng.bytes(90_000), rng.bytes(300_000));
    std::fs::write(dir.join("kernel.efi"), &kernel).unwrap();
    std::fs::write(dir.join("system.img"), &system).unwrap();
    if !dir.join("release.secret").exists() {
        let (secret, public) = (p("release.secret"), p("release.ed25519"));
        let text = sign_line(&format!("keygen --secret {secret} --public {public}"));
        assert!(text.contains("Key id: "), "{text}");
    }
    let root = "5a".repeat(32);
    let kernel_arg = format!("--payload kernel={}", p("kernel.efi"));
    let system_arg = format!("--payload system={}:{root}", p("system.img"));
    write_manifest(dir, version, index, &format!("{system_arg} {kernel_arg}"));
    let (manifest, secret, signed) = (p("manifest.txt"), p("release.secret"), p("manifest.sig"));
    sign_line(&format!(
        "sign --manifest {manifest} --key {secret} --out {signed}"
    ));
    let (out, system_plain) = (p("release.uup"), p("system.img"));
    sign_line(&format!(
        "bundle --signed {signed} --out {out} {kernel_arg} --payload system={system_plain}"
    ));
    Publisher {
        dir: dir.to_path_buf(),
        kernel,
        system,
    }
}

#[test]
fn signing_tool_builds_a_bundle_that_verifies_and_keeps_the_secret_private() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = publish(dir.path(), "0.7.1", "2");
    let keys = publisher.key_dir("keys");
    let bundle = publisher.path("release.uup");
    let text = sign_line(&format!(
        "verify --bundle {bundle} --keys {}",
        keys.display()
    ));
    assert!(text.contains("version 0.7.1, rollback index 2"), "{text}");

    // The secret key never appears in any output and is private on disk.
    let secret = std::fs::read_to_string(dir.path().join("release.secret")).unwrap();
    for name in ["manifest.txt", "manifest.sig", "release.uup"] {
        let bytes = std::fs::read(dir.path().join(name)).unwrap();
        let hay = String::from_utf8_lossy(&bytes);
        assert!(
            !hay.contains(secret.trim()),
            "{name} contains the secret key"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("release.secret"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // `uupdate verify` (the device-side command) agrees with the tool.
    let (code, text) = run_line(CLI, &format!("verify {bundle} --keys {}", keys.display()));
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("version 0.7.1"), "{text}");
}

#[test]
fn signing_tool_failures_have_distinct_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = publish(dir.path(), "0.7.1", "2");
    let p = |name: &str| publisher.path(name);
    let (manifest, secret, bundle) = (p("manifest.txt"), p("release.secret"), p("release.uup"));
    // Usage errors: 2.
    assert_eq!(run(SIGN, &[]).0, 2);
    assert_eq!(run_line(SIGN, "frobnicate").0, 2);
    assert_eq!(run_line(SIGN, &format!("sign --manifest {manifest}")).0, 2);
    let with_inline_secret = format!(
        "sign --manifest {manifest} --key {secret} --out {} --secret-key abcd",
        p("x")
    );
    assert_eq!(run_line(SIGN, &with_inline_secret).0, 2);
    // Refused overwrite of a key and wrong inputs: 1.
    let overwrite = format!("keygen --secret {secret} --public {}", p("other.pub"));
    assert_eq!(run_line(SIGN, &overwrite).0, 1);
    let not_a_manifest = format!("sign --manifest {bundle} --key {secret} --out {}", p("x"));
    assert_eq!(run_line(SIGN, &not_a_manifest).0, 1);
    let empty_keys = dir.path().join("no-keys");
    std::fs::create_dir_all(&empty_keys).unwrap();
    let verify = format!("verify --bundle {bundle} --keys {}", empty_keys.display());
    let (code, text) = run_line(SIGN, &verify);
    assert_eq!(code, 1);
    assert!(text.contains("no trusted keys"), "{text}");
}

#[test]
fn signing_tool_rotation_is_accepted_by_a_device() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = publish(dir.path(), "0.7.1", "2");
    let p = |name: &str| publisher.path(name);
    let keys = publisher.key_dir("keys");
    // Rotation to a second key, signed by the first.
    let (next_secret, next_public) = (p("next.secret"), p("next.ed25519"));
    sign_line(&format!(
        "keygen --secret {next_secret} --public {next_public}"
    ));
    sign_line(&format!(
        "rotate --serial 1 --created 1795000001 --key {} --out {} --public {next_public}",
        p("release.secret"),
        p("rotation.sig")
    ));
    let mut trust = ultar_update::keys::TrustStore::open(&keys, &dir.path().join("state")).unwrap();
    assert!(trust
        .accept(&std::fs::read(dir.path().join("rotation.sig")).unwrap())
        .unwrap());
}

struct Daemon(Child);

impl Daemon {
    /// Starts the daemon and waits until it reports that the service is registered.
    fn start(config: &Path, socket: &Path) -> Self {
        let (config, socket) = (config.display().to_string(), socket.display().to_string());
        let mut child = Command::new(DAEMON)
            .args(["--config", &config, "--socket", &socket, "--lang", "en"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stderr.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "Update service ready");
        Self(child)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A device for the daemon: burned-in key, file-based slots with a factory system, the
/// configuration file, and a bus with one socket per identity.
struct DaemonDevice {
    boot: FileBootControl,
    config: PathBuf,
    sockets: PathBuf,
    _broker: Broker,
}

impl DaemonDevice {
    fn new(publisher: &Publisher, server: &str) -> Self {
        let dir = &publisher.dir;
        let keys = publisher.key_dir("update-keys");
        let boot_dir = dir.join("boot");
        let boot = FileBootControl::create(&boot_dir, &[Role::Kernel, Role::System]).unwrap();
        for role in [Role::Kernel, Role::System] {
            let path = boot.image_path(Slot::A, role);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"factory").unwrap();
        }
        let config = dir.join("update.conf");
        let text = format!(
            "device=vm\narch=x86_64\nedition=desktop\nversion=0.7.0\nrollback_index=1\n\
             roles=kernel,system\nboot_dir={}\nstate_dir={}\nkeys_dir={}\nserver={server}\n\
             admin=system.settings\nreserve_mib=1\nhealth_service=none\n",
            boot_dir.display(),
            dir.join("state").display(),
            keys.display(),
        );
        std::fs::write(&config, text).unwrap();
        // The bus: one socket per identity (the broker binds identity to the socket).
        let sockets = dir.join("bus");
        std::fs::create_dir_all(&sockets).unwrap();
        let mut builder = Broker::builder(Policy::allow_all());
        for identity in ["system.updated", "system.settings", "app.files"] {
            let auth = Arc::new(FixedIdentity::new(Identity::new(identity).unwrap()));
            builder = builder.listen(&sockets.join(identity), auth);
        }
        Self {
            boot,
            config,
            sockets,
            _broker: builder.start().unwrap(),
        }
    }

    fn start_daemon(&self) -> Daemon {
        Daemon::start(&self.config, &self.sockets.join("system.updated"))
    }

    /// Runs `uupdate <command>` as `identity`.
    fn cli(&self, identity: &str, command: &str) -> (i32, String) {
        let socket = self.sockets.join(identity).display().to_string();
        run_line(CLI, &format!("{command} --socket {socket}"))
    }

    /// Download as a user, refused install as a user, install as an administrator.
    fn download_and_install(&self, publisher: &Publisher) {
        let (code, text) = self.cli("app.files", "status");
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("Running version: 0.7.0 (slot a)"), "{text}");
        // No network service reports here: the connection counts as metered, and only the
        // administrator can decide to use it.
        let (code, text) = self.cli("app.files", "download --metered");
        assert_eq!(code, 1, "{text}");
        let (code, text) = self.cli("system.settings", "download --metered");
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("The update is ready to install"), "{text}");
        assert!(text.contains("Important security update"), "{text}");
        let (code, text) = self.cli("app.files", "apply");
        assert_eq!(code, 1, "{text}");
        assert!(text.contains("administrator rights"), "{text}");
        let (code, text) = self.cli("system.settings", "apply");
        assert_eq!(code, 0, "{text}");
        assert!(
            text.contains("Restart the device to finish the update"),
            "{text}"
        );
        let image = |role| std::fs::read(self.boot.image_path(Slot::B, role)).unwrap();
        assert_eq!(image(Role::Kernel), publisher.kernel);
        assert_eq!(image(Role::System), publisher.system);
        assert_eq!(self.boot.verity_root(Slot::B).unwrap(), Some([0x5a; 32]));
    }
}

#[test]
fn daemon_process_installs_a_release_from_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = publish(dir.path(), "0.7.1", "2");
    let bundle = std::fs::read(dir.path().join("release.uup")).unwrap();
    let server = TestServer::start(Behaviour {
        file: bundle.clone(),
        latest: Some(TestServer::latest_json(bundle.len() as u64)),
        ..Behaviour::default()
    });
    let device = DaemonDevice::new(&publisher, &server.base);

    let daemon = device.start_daemon();
    device.download_and_install(&publisher);
    drop(daemon);

    // "Restart": the boot code chooses the new slot; the daemon confirms it after its start.
    assert_eq!(device.boot.simulate_boot().unwrap(), Slot::B);
    let _daemon = device.start_daemon();
    let (code, text) = device.cli("app.files", "status");
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Running version: 0.7.1 (slot b)"), "{text}");
    assert!(text.contains("Security level: 2"), "{text}");
    let state = device.boot.slot_state(Slot::B).unwrap();
    assert!(state.successful && state.is_default);
    let (_, text) = device.cli("app.files", "history");
    assert!(text.contains("System update completed"), "{text}");
    // The requests the daemon made: no account, header probe, then the body.
    let seen = server.seen();
    assert_eq!(
        seen[0].target,
        "/v1/update/latest?channel=stable&arch=x86_64&edition=desktop"
    );
    assert!(seen.iter().all(|s| s.authorization.is_none()));
}

#[test]
fn daemon_refuses_to_start_on_bad_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("update.conf");
    let (code, text) = run(DAEMON, &[]);
    assert_eq!(code, 2);
    assert!(text.contains("Usage: ultar-updated"), "{text}");
    std::fs::write(&config, "device=vm\narch=x86_64\nprotect=off\n").unwrap();
    let (code, text) = run(
        DAEMON,
        &[
            "--config",
            &config.display().to_string(),
            "--socket",
            "/nonexistent/bus",
        ],
    );
    assert_eq!(code, 1);
    assert!(text.contains("invalid (line 3)"), "{text}");
    // A valid file but no slot table: typed failure, not a guess.
    std::fs::write(
        &config,
        format!("device=vm\narch=x86_64\nedition=desktop\nversion=0.7.0\nroles=kernel\nboot_dir={0}/b\nstate_dir={0}/s\nkeys_dir={0}/k\nhealth_service=none\n", dir.path().display()),
    )
    .unwrap();
    let (code, text) = run(
        DAEMON,
        &[
            "--config",
            &config.display().to_string(),
            "--socket",
            "/nonexistent/bus",
        ],
    );
    assert_eq!(code, 1);
    assert!(text.contains("boot data could not be read"), "{text}");
}
