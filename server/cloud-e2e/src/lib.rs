//! Test harness for end-to-end tests of the cloud client against the real server (area: tests).
//!
//! [`Stack`] starts `ultraos-server` in-process on an ephemeral port with a manual clock, owns a
//! publisher key pair and a trusted-keys directory like a device has, and can publish signed
//! `.uap` packages. Nothing here is used in production.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_panics_doc)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use ultar_cloud::Client;
use ultraos_server::admin::{self, AppPublication, ReleasePublication};
use ultraos_server::clock::ManualClock;
use ultraos_server::ratelimit::{BucketConfig, LimiterConfig};
use ultraos_server::{AppState, Config};

/// A running server plus a simulated device environment.
pub struct Stack {
    /// Address the server listens on.
    pub addr: SocketAddr,
    /// Server state, for administrative calls.
    pub state: Arc<AppState>,
    /// The clock that drives rate limiting.
    pub clock: Arc<ManualClock>,
    /// Temporary directory holding everything.
    pub dir: tempfile::TempDir,
    /// Private key of the trusted publisher (hex).
    pub publisher_key: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Stack {
    /// Starts a server with limits far above what tests use.
    pub fn start() -> Self {
        let roomy = BucketConfig {
            capacity: 1_000_000,
            refill_per_minute: 1_000_000,
        };
        Self::start_with(LimiterConfig {
            general: roomy,
            download: roomy,
            auth: roomy,
            max_entries: 1000,
        })
    }

    /// Starts a server with the given rate limits.
    pub fn start_with(limits: LimiterConfig) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("server.sqlite");
        let mut config = Config::new(db.to_string_lossy(), dir.path().join("files"), [0x33; 32]);
        config.limits = limits;
        config.wg_server_pubkey = "server-pubkey".into();
        let clock = Arc::new(ManualClock::new(0));
        let state = AppState::new(config, clock.clone()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let served = state.clone();
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let stop = async {
                    let _ = rx.await;
                };
                ultraos_server::serve(listener, served, stop).await.unwrap();
            });
        });
        let (publisher_key, public_key) = ultar_pkg::keygen();
        std::fs::create_dir_all(dir.path().join("trusted")).unwrap();
        std::fs::write(dir.path().join("trusted/kmcodecraft.ed25519"), public_key).unwrap();
        Self {
            addr,
            state,
            clock,
            dir,
            publisher_key,
            shutdown: Some(tx),
            thread: Some(thread),
        }
    }

    /// Server address as a URL.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// A signed-out client.
    pub fn anonymous(&self) -> Client {
        Client::new(&self.base(), None)
    }

    /// Registers an account and returns a signed-in client.
    pub fn signed_in(&self, username: &str) -> Client {
        let token = self.anonymous().register(username, "password-123").unwrap();
        Client::new(&self.base(), Some(token))
    }

    /// The device's trusted-keys directory.
    pub fn trusted(&self) -> PathBuf {
        self.dir.path().join("trusted")
    }

    /// The simulated device root for installations.
    pub fn root(&self) -> PathBuf {
        self.dir.path().join("device-root")
    }

    /// Download work directory of the device.
    pub fn work(&self) -> PathBuf {
        self.dir.path().join("dl")
    }

    /// Builds a `.uap` with manifest id `id`, signed with `key`; returns its path.
    pub fn build_package(&self, id: &str, version: &str, key: &str) -> PathBuf {
        let src = self.dir.path().join(format!("src-{id}-{version}"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.txt"), format!("{id} {version}")).unwrap();
        let manifest = self.dir.path().join(format!("{id}-{version}.toml"));
        let text = format!(
            "id = \"{id}\"\nname = \"App {id}\"\nversion = \"{version}\"\nkind = \"data\"\npermissions = [\"network\"]\npublisher = \"KM Code Craft\"\n"
        );
        std::fs::write(&manifest, text).unwrap();
        let out = self.dir.path().join(format!("{id}-{version}.uap"));
        ultar_pkg::pack_and_sign(&manifest, &src, &out, key).unwrap();
        out
    }

    /// Publishes `package` in the store under `listed_id` (which may differ from the manifest).
    pub fn publish(&self, listed_id: &str, version: &str, premium: bool, package: &Path) -> String {
        let p: AppPublication = serde_json::from_value(serde_json::json!({
            "id": listed_id, "name": format!("App {listed_id}"), "version": version,
            "description": "An example app", "category": "office", "publisher": "KM Code Craft",
            "premium": premium, "permissions": ["network"], "file_types": ["txt"]
        }))
        .unwrap();
        admin::publish_app(&self.state.db, &self.state.files, &p, package)
            .unwrap()
            .sha256
    }

    /// Builds, signs with the trusted publisher key and publishes an app.
    pub fn publish_signed(&self, id: &str, version: &str, premium: bool) -> String {
        let package = self.build_package(id, version, &self.publisher_key);
        self.publish(id, version, premium, &package)
    }

    /// Publishes a system image; returns the release id.
    pub fn publish_release(&self, channel: &str, version: &str, content: &[u8]) -> i64 {
        let p: ReleasePublication = serde_json::from_value(serde_json::json!({
            "channel": channel, "version": version, "arch": std::env::consts::ARCH,
            "roothash": "ab12", "sig": "c2ln"
        }))
        .unwrap();
        let file = self.dir.path().join("image.bin");
        std::fs::write(&file, content).unwrap();
        admin::publish_release(&self.state.db, &self.state.files, &p, &file)
            .unwrap()
            .0
    }

    /// True when nothing was installed on the simulated device.
    pub fn nothing_installed(&self) -> bool {
        std::fs::read_dir(self.root())
            .map(|mut d| d.next().is_none())
            .unwrap_or(true)
    }

    /// True when the download work directory holds no leftover files.
    pub fn no_leftover_downloads(&self) -> bool {
        std::fs::read_dir(self.work())
            .map(|mut d| d.next().is_none())
            .unwrap_or(true)
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
