//! Every shipped service file runs with the least it needs: its own user
//! unless an explicit, justified allow-list says why it needs root, never
//! `all` rights, every kept right justified in the file, and every right
//! at once only for the administrator shells.

use std::fs;
use std::path::{Path, PathBuf};
use ultar_init::spec::{ServiceSpec, ALL_RIGHTS, RIGHT_NAMES};

/// Services that still run as root, with the exact reason (the file
/// repeats it in a `# root:` comment).
const ROOT_ALLOWED: [(&str, &str); 17] = [
    ("compat-bind", "bind mount of the compat tree"),
    ("drivers", "module loading (finit_module) and /sys driver files"),
    ("entwickler", "remote administrator shell (developer access)"),
    ("fingerabdruck", "vendor HAL in a chroot with bind mounts and chmod of device nodes"),
    ("gnss", "vendor HAL in a chroot with bind mounts and chmod of /dev/gnss_ipc"),
    ("llm-compat", "the compat runner unshares, mounts and chroots as root"),
    ("modem", "firmware into /dev/umts_boot0, module parameters, EFS mount"),
    ("net", "addresses and routes, DHCP on port 68, wpa_supplicant via the compat runner"),
    ("netzregeln", "firewall rules"),
    ("ril", "modem IPC devices, address and routes of the mobile data interface"),
    ("selftest", "root-only control socket, dpkg and programs in the compat tree"),
    ("shell", "emergency administrator shell at the console"),
    ("tontest", "root-only control socket (power off after the tone)"),
    ("ui", "the surface still administers the system itself (drivers, Wi-Fi, cells, packages, clock, reboot)"),
    ("verbund", "screen and file exchange through root-only files of the surface in /run/ultar"),
    ("webtest", "root-only control socket and the compat runner"),
    ("wifitest", "access point, interface address, /proc/sys switches via the compat runner"),
];

/// Services that keep every right: administrator shells and the surface
/// (until its administration moves behind the bus).
const FULL_RIGHTS_ALLOWED: [&str; 3] = ["entwickler", "shell", "ui"];

fn shipped() -> Vec<(PathBuf, String, ServiceSpec)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/services");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.retain(|path| path.extension().is_some_and(|ext| ext == "toml"));
    files.sort();
    assert!(
        files.len() >= 20,
        "shipped service files not found in {dir:?}"
    );
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap_or_default();
            let spec = ServiceSpec::load(&path).unwrap_or_else(|e| panic!("{path:?}: {e:?}"));
            (path, text, spec)
        })
        .collect()
}

fn is_root(spec: &ServiceSpec) -> bool {
    matches!(spec.confinement.uid, None | Some(0))
}

#[test]
fn no_shipped_file_grants_all_rights() {
    for (path, text, spec) in shipped() {
        assert!(!spec.confinement.all_rights, "{path:?} grants all");
        let rights_lines = text
            .lines()
            .filter(|l| l.trim_start().starts_with("rights"));
        for line in rights_lines {
            assert!(!line.contains("\"all\""), "{path:?}: {line}");
        }
    }
}

#[test]
fn every_shipped_service_has_its_own_user_unless_justified() {
    for (path, text, spec) in shipped() {
        let allowed = ROOT_ALLOWED.iter().any(|(name, _)| *name == spec.name);
        if is_root(&spec) {
            assert!(
                allowed,
                "{path:?} runs as root but is not on the allow-list"
            );
            assert!(
                text.contains("# root: "),
                "{path:?} lacks its `# root:` reason"
            );
        } else {
            assert!(
                !allowed,
                "{} is allow-listed for root but has a user",
                spec.name
            );
        }
    }
}

#[test]
fn every_kept_right_is_justified_and_every_right_only_for_admin_shells() {
    for (path, text, spec) in shipped() {
        let mask = spec.confinement.rights;
        for (bit, name) in RIGHT_NAMES.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                let reason = format!("# right {name}: ");
                assert!(
                    text.contains(&reason),
                    "{path:?} keeps {name} without a reason"
                );
            }
        }
        if mask == ALL_RIGHTS {
            assert!(
                FULL_RIGHTS_ALLOWED.contains(&spec.name.as_str()),
                "{path:?}"
            );
        }
    }
}

#[test]
fn the_allow_lists_name_only_shipped_services() {
    let names: Vec<String> = shipped()
        .into_iter()
        .map(|(_, _, spec)| spec.name)
        .collect();
    for (name, reason) in ROOT_ALLOWED {
        assert!(names.iter().any(|n| n == name), "stale root entry {name}");
        assert!(!reason.is_empty());
    }
    for name in FULL_RIGHTS_ALLOWED {
        assert!(
            names.iter().any(|n| n == name),
            "stale full-rights entry {name}"
        );
    }
}

#[test]
fn the_heartbeat_and_the_language_model_run_without_root() {
    let specs: Vec<ServiceSpec> = shipped().into_iter().map(|(_, _, spec)| spec).collect();
    let find = |name: &str| {
        specs
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.confinement.clone())
    };
    let heartbeat = find("heartbeat").unwrap_or_default();
    assert_eq!(
        (heartbeat.uid, heartbeat.gid, heartbeat.rights),
        (Some(65534), Some(65534), 0)
    );
    let llm = find("llm").unwrap_or_default();
    assert_eq!((llm.uid, llm.rights), (Some(810), 0));
}
