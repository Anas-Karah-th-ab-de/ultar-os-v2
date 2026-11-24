//! Test support for the process tests of the services (feature `testkit`, never in a
//! program): an in-process broker whose identities are tokens, stand-in services, and
//! the real service binaries as child processes (with `--allow-root` when the test runs
//! as root, so the same tests run as root and as a plain user).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::{Broker, Client, ClientOptions, Identity, IncomingRequest, Policy, TokenIdentity};

/// Longest wait of a test step.
pub const WAIT: Duration = Duration::from_secs(20);

/// Polls `f` until it is true or `timeout` passed.
pub fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

/// A broker, a directory and the stand-ins of one test.
pub struct World {
    broker: Option<Broker>,
    pub socket: PathBuf,
    pub dir: PathBuf,
    stop: Arc<AtomicBool>,
}

/// A service binary running as a child process.
pub struct Daemon(pub Child);

impl Daemon {
    /// Sends `SIGTERM` and returns the exit code (`None`: killed or still running after `WAIT`).
    pub fn terminate(&mut self) -> Option<i32> {
        let _ = Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let end = Instant::now() + WAIT;
        while Instant::now() < end {
            if let Ok(Some(status)) = self.0.try_wait() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    /// Ends it at once (`SIGKILL`), like a crash.
    pub fn crash(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        drop(self.broker.take());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// True when the test runs as root.
pub fn is_root() -> bool {
    ultar_service::effective_uid() == 0
}

impl World {
    /// A broker with `policy`; every name in `identities` is its own token.
    pub fn new(tag: &str, policy: &str, identities: &[&str]) -> World {
        let dir = std::env::temp_dir().join(format!("ultar-svc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.join("bus.sock");
        let mut auth = TokenIdentity::new();
        for name in identities {
            auth = auth.with_token(name, Identity::new(name).unwrap());
        }
        let broker = Broker::builder(Policy::from_toml_str(policy).unwrap())
            .listen(&socket, Arc::new(auth))
            .start()
            .unwrap();
        std::fs::write(dir.join("policy.toml"), policy).unwrap();
        World {
            broker: Some(broker),
            socket,
            dir,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The policy file of this world.
    pub fn policy_file(&self) -> PathBuf {
        self.dir.join("policy.toml")
    }

    /// A sub-directory (created, mode 0700).
    pub fn subdir(&self, name: &str) -> PathBuf {
        let d = self.dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    /// A client with `identity`.
    pub fn client(&self, identity: &str) -> Client {
        connect(&self.socket, identity)
    }

    /// Starts `program` as `identity` with `args` (plus `--bus`, `--token-file` and,
    /// as root, `--allow-root`).
    pub fn spawn(&self, program: &str, identity: &str, args: &[&str]) -> Daemon {
        let token = self.dir.join(format!("{identity}.token"));
        std::fs::write(&token, identity).unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut cmd = Command::new(program);
        cmd.arg("--bus")
            .arg(&self.socket)
            .arg("--token-file")
            .arg(&token)
            .args(args)
            .stdin(Stdio::null());
        if is_root() {
            cmd.arg("--allow-root");
        }
        Daemon(cmd.spawn().unwrap())
    }

    /// A stand-in service: `handle` answers every call to `service`.
    pub fn serve(
        &self,
        identity: &str,
        service: &str,
        mut handle: impl FnMut(IncomingRequest) + Send + 'static,
    ) {
        let client = self.client(identity);
        let endpoint = client.register(service).unwrap();
        let stop = self.stop.clone();
        std::thread::spawn(move || {
            let _keep = client;
            while !stop.load(Ordering::SeqCst) {
                if let Ok(r) = endpoint.recv_timeout(Duration::from_millis(50)) {
                    handle(r);
                }
            }
        });
    }
}

/// Connects to `socket` with `identity` as token.
pub fn connect(socket: &Path, identity: &str) -> Client {
    let options = ClientOptions {
        token: Some(identity.to_owned()),
        ..ClientOptions::named(identity)
    };
    Client::connect(socket, options).unwrap()
}
