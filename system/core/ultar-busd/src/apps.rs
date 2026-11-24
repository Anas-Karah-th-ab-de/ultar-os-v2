//! Identity of apps on the apps' socket (`--listen-apps`).
//!
//! Every app runs as its own user id, allocated by the launcher from the app
//! range (`--app-uids FIRST-LAST`). The launcher writes the file
//! `--app-map` with one line `UID = "app.<id>"` for every installed app whose
//! signed manifest declares bus access. A connecting process is the identity
//! its **kernel credentials** (`SO_PEERCRED`) map to; the policy then applies
//! per app id (rules `app.settings`, `app.store` …, plus `app.*`).
//!
//! Default deny, fail closed:
//! * a user id outside the app range is refused (no service, no root can
//!   pose as an app here – services have their own sockets);
//! * a user id without line (an app without bus declarations, or not
//!   installed) is refused;
//! * the file must be a regular file owned by root or by `--app-map-owner`
//!   (the launcher), not writable by group or others, at most
//!   [`MAX_MAP_BYTES`]; every line must name a uid inside the range and an
//!   identity `app.<id>` (never `app.*`-like patterns, never `system.…`). A
//!   file that breaks any of these gives **no** app an identity until it is
//!   fixed.
//!
//! The file is read again when it changed (device, inode, size or
//! modification time differ), checked at every connect, so an app installed
//! after the broker started is known at its first connection without a
//! signal.

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Mutex;
use ultar_bus::{BusError, Identity, PeerIdentity, PeerInfo};
use ultar_service::peercred::{peer_credentials, MAX_MAP_BYTES};
use ultar_service::UidMap;

/// Prefix every app identity has.
pub const APP_PREFIX: &str = "app.";

/// Where the map comes from and what it may contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMapSource {
    /// The launcher's file.
    pub path: PathBuf,
    /// The user that may own the file besides root (the launcher).
    pub owner: u32,
    /// First and last app user id.
    pub uids: (u32, u32),
}

/// What identifies a version of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
}

/// Why the file was refused (kept for the tests and diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// Missing or unreadable (no app installed yet, or the launcher withdrew it).
    Missing,
    /// Wrong owner, type or mode.
    Unsafe,
    /// Too large or not `UID = "identity"` lines.
    Syntax,
    /// A uid outside the range or an identity that is no `app.<id>`.
    Foreign,
}

/// Checks the parsed entries: uids in range, identities `app.<id>`.
pub fn check_entries(map: &UidMap, text: &str, uids: (u32, u32)) -> Result<(), MapError> {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((uid, _)) = line.split_once('=') else {
            continue;
        };
        let uid: u32 = uid.trim().parse().map_err(|_| MapError::Syntax)?;
        if uid < uids.0 || uid > uids.1 {
            return Err(MapError::Foreign);
        }
        let identity = map.identity(uid).ok_or(MapError::Syntax)?;
        let name = identity.as_str();
        let Some(id) = name.strip_prefix(APP_PREFIX) else {
            return Err(MapError::Foreign);
        };
        if id.is_empty() || id == "shared" || id.contains('*') {
            return Err(MapError::Foreign);
        }
    }
    Ok(())
}

fn load(source: &AppMapSource) -> Result<(Stamp, UidMap), MapError> {
    let file = std::fs::File::open(&source.path).map_err(|_| MapError::Missing)?;
    let meta = file.metadata().map_err(|_| MapError::Missing)?;
    if !meta.is_file() || (meta.uid() != 0 && meta.uid() != source.owner) {
        return Err(MapError::Unsafe);
    }
    if meta.mode() & 0o022 != 0 {
        return Err(MapError::Unsafe);
    }
    if meta.len() > MAX_MAP_BYTES {
        return Err(MapError::Syntax);
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(file, MAX_MAP_BYTES + 1), &mut text)
        .map_err(|_| MapError::Syntax)?;
    if text.len() as u64 > MAX_MAP_BYTES {
        return Err(MapError::Syntax);
    }
    let map = UidMap::parse(&text).map_err(|_| MapError::Syntax)?;
    check_entries(&map, &text, source.uids)?;
    Ok((stamp(&meta), map))
}

fn stamp(meta: &std::fs::Metadata) -> Stamp {
    Stamp {
        dev: meta.dev(),
        ino: meta.ino(),
        size: meta.len(),
        mtime: meta.mtime(),
        mtime_ns: meta.mtime_nsec(),
    }
}

fn current_stamp(source: &AppMapSource) -> Option<Stamp> {
    std::fs::metadata(&source.path).ok().map(|m| stamp(&m))
}

/// The authenticator of the apps' socket.
pub struct AppIdentity {
    source: AppMapSource,
    state: Mutex<(Option<Stamp>, UidMap)>,
}

impl AppIdentity {
    /// No app is known until the file is read at the first connect.
    pub fn new(source: AppMapSource) -> Self {
        Self {
            source,
            state: Mutex::new((None, UidMap::new())),
        }
    }

    /// The identity of `uid` with the file as it is now (reads it again when it changed).
    pub fn identity_of(&self, uid: u32) -> Result<Identity, BusError> {
        if uid < self.source.uids.0 || uid > self.source.uids.1 {
            return Err(BusError::AuthFailed);
        }
        let Ok(mut state) = self.state.lock() else {
            return Err(BusError::AuthFailed);
        };
        let now = current_stamp(&self.source);
        if now.is_none() || now != state.0 {
            *state = match load(&self.source) {
                Ok((stamp, map)) => (Some(stamp), map),
                // fail closed: nobody until the file is good again (read again at the next connect)
                Err(_) => (None, UidMap::new()),
            };
        }
        state.1.identity(uid).cloned().ok_or(BusError::AuthFailed)
    }
}

impl PeerIdentity for AppIdentity {
    fn identify(&self, peer: &PeerInfo<'_>) -> Result<Identity, BusError> {
        let credentials = peer_credentials(peer.stream).map_err(|_| BusError::AuthFailed)?;
        self.identity_of(credentials.uid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("busd-apps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: &std::path::Path, text: &str, mode: u32) {
        // a new file each time (as the launcher's atomic rename): a new inode
        let tmp = path.with_extension("new");
        std::fs::write(&tmp, text).unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)).unwrap();
        std::fs::rename(&tmp, path).unwrap();
    }

    fn me() -> u32 {
        // one probe file per call: the tests run in parallel and remove their probe again
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let probe = std::env::temp_dir().join(format!("busd-apps-me-{}-{n}", std::process::id()));
        std::fs::write(&probe, b"").unwrap();
        let uid = std::fs::metadata(&probe).unwrap().uid();
        let _ = std::fs::remove_file(&probe);
        uid
    }

    fn source(path: PathBuf) -> AppMapSource {
        AppMapSource {
            path,
            owner: me(),
            uids: (20000, 49999),
        }
    }

    #[test]
    fn each_app_is_its_own_identity_and_others_are_refused() {
        let d = dir("ok");
        let path = d.join("bus-apps.toml");
        let apps = AppIdentity::new(source(path.clone()));
        // no file yet: nobody
        assert!(apps.identity_of(20001).is_err());
        write(
            &path,
            "# launcher\n20001 = \"app.settings\"\n20002 = \"app.store\"\n",
            0o644,
        );
        assert_eq!(apps.identity_of(20001).unwrap().as_str(), "app.settings");
        assert_eq!(apps.identity_of(20002).unwrap().as_str(), "app.store");
        // an app without bus declarations has no line
        assert!(apps.identity_of(20003).is_err());
        // outside the app range: never, even if the file named it
        assert!(apps.identity_of(805).is_err());
        assert!(apps.identity_of(0).is_err());
        // installed later: known at the next connect, without a signal
        write(
            &path,
            "20001 = \"app.settings\"\n20003 = \"app.files\"\n",
            0o644,
        );
        assert_eq!(apps.identity_of(20003).unwrap().as_str(), "app.files");
        assert!(apps.identity_of(20002).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bad_file_gives_no_app_an_identity() {
        let d = dir("bad");
        let path = d.join("bus-apps.toml");
        let apps = AppIdentity::new(source(path.clone()));
        let bad = [
            ("20001 = \"app.settings\"\n", 0o666),    // writable by others
            ("20001 = \"app.settings\"\n", 0o664),    // writable by the group
            ("20001 = \"system.admin\"\n", 0o644),    // not an app
            ("20001 = \"app.shared\"\n", 0o644),      // the old shared identity
            ("805 = \"app.settings\"\n", 0o644),      // a service's uid
            ("20001 = \"app.settings\"\nx\n", 0o644), // syntax
            ("20001 = \"app.a\"\n20001 = \"app.b\"\n", 0o644), // twice
        ];
        for (text, mode) in bad {
            write(&path, "20001 = \"app.settings\"\n", 0o644);
            assert!(apps.identity_of(20001).is_ok());
            write(&path, text, mode);
            assert!(apps.identity_of(20001).is_err(), "{text:?} {mode:o}");
        }
        std::fs::remove_file(&path).unwrap();
        assert!(apps.identity_of(20001).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_of_a_stranger_is_refused() {
        let d = dir("owner");
        let path = d.join("bus-apps.toml");
        write(&path, "20001 = \"app.settings\"\n", 0o644);
        let mut s = source(path);
        // the file belongs to us; require another owner (and we are not root in the test, or are)
        s.owner = me().wrapping_add(1);
        let apps = AppIdentity::new(s);
        if me() == 0 {
            assert!(apps.identity_of(20001).is_ok()); // root may always own it
        } else {
            assert!(apps.identity_of(20001).is_err());
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn hostile_files_never_panic_and_never_name_a_system_identity() {
        // fixed-seed generator: random bytes and near-valid lines
        let mut seed: u64 = 0x5eed_a995;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let parts = [
            "20001",
            "805",
            "0",
            " = ",
            "\"app.x\"",
            "\"system.admin\"",
            "\"app.*\"",
            "#",
            "\n",
            "=",
            "\"",
            "app.",
            "49999",
            "50000",
        ];
        for _ in 0..2000 {
            let mut text = String::new();
            for _ in 0..(next() % 12) {
                text.push_str(parts[(next() % parts.len() as u64) as usize]);
            }
            if let Ok(map) = UidMap::parse(&text) {
                if check_entries(&map, &text, (20000, 49999)).is_ok() {
                    for uid in [0, 805, 20001, 49999, 50000] {
                        if let Some(id) = map.identity(uid) {
                            assert!((20000..=49999).contains(&uid));
                            assert!(id.as_str().starts_with(APP_PREFIX));
                            assert!(!id.as_str().contains('*'));
                        }
                    }
                }
            }
        }
    }

    /// End to end on a real broker with the shipped policy: the peer is the app its uid maps to;
    /// a call outside that app's rule is refused, an app without a line cannot connect at all.
    #[test]
    fn a_real_broker_applies_the_policy_per_app_and_refuses_apps_without_bus_access() {
        use std::sync::Arc;
        use std::time::Duration;
        use ultar_bus::messages::settings::{Get, GetReply};
        use ultar_bus::{
            Broker, Client, ClientOptions, ErrorCode, FixedIdentity, Policy, SocketAccess,
        };
        let d = dir("e2e");
        let map = d.join("bus-apps.toml");
        let uid = me();
        let policy = Policy::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../rootfs/etc/ultar/bus-policy.toml"),
        )
        .unwrap();
        let apps_sock = d.join("apps.sock");
        let svc_sock = d.join("svc.sock");
        let apps = AppIdentity::new(AppMapSource {
            path: map.clone(),
            owner: uid,
            uids: (uid, uid),
        });
        let broker = Broker::builder(policy)
            .listen_with_access(&apps_sock, Arc::new(apps), SocketAccess::default())
            .listen_with_access(
                &svc_sock,
                Arc::new(FixedIdentity::new(
                    Identity::new("system.settingsd").unwrap(),
                )),
                SocketAccess::default(),
            )
            .start()
            .unwrap();
        let options = || ClientOptions {
            connect_retry_for: Duration::ZERO,
            ..ClientOptions::named("test-app")
        };
        let service = Client::connect(&svc_sock, options()).unwrap();
        let endpoint = service.register("settings").unwrap();
        std::thread::spawn(move || {
            endpoint.run(|request| {
                let _ = request.respond::<Get>(|_| Ok(GetReply { value: None }));
            });
        });
        let get = || Get {
            key: "appearance.theme".into(),
        };
        let wait = Duration::from_secs(5);
        // the Files app: no settings right
        write(&map, &format!("{uid} = \"app.files\"\n"), 0o644);
        let files = Client::connect(&apps_sock, options()).unwrap();
        assert!(matches!(
            files.call(&get(), wait),
            Err(BusError::Remote(ErrorCode::PermissionDenied))
        ));
        // the same uid as the Settings app: its page's call goes through
        write(&map, &format!("{uid} = \"app.settings\"\n"), 0o644);
        let settings = Client::connect(&apps_sock, options()).unwrap();
        assert!(settings.call(&get(), wait).is_ok());
        // an app without bus declarations has no line: not even a connection
        write(&map, "# no app declares bus access\n", 0o644);
        assert!(Client::connect(&apps_sock, options()).is_err());
        // the clients before the service, the service before the broker
        drop(settings);
        drop(files);
        drop(service);
        drop(broker);
        let _ = std::fs::remove_dir_all(&d);
    }
}
