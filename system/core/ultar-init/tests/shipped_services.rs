//! Every shipped service file runs with the least it needs: its own user
//! unless an explicit, justified allow-list says why it needs root, never
//! `all` rights, every kept right justified in the file, and every right
//! at once only for the administrator shells.

use std::fs;
use std::path::{Path, PathBuf};
use ultar_init::spec::{ServiceSpec, ALL_RIGHTS, RIGHT_NAMES};

/// Services that still run as root, with the exact reason (the file
/// repeats it in a `# root:` comment).
const ROOT_ALLOWED: [(&str, &str); 20] = [
    ("compat-bind", "bind mount of the compat tree"),
    ("drivers", "module loading (finit_module) and /sys driver files"),
    ("entwickler", "remote administrator shell (developer access)"),
    ("fingerabdruck", "vendor HAL in a chroot with bind mounts and chmod of device nodes"),
    ("gnss", "vendor HAL in a chroot with bind mounts and chmod of /dev/gnss_ipc"),
    ("llm-compat", "the compat runner unshares, mounts and chroots as root"),
    ("modem", "firmware into /dev/umts_boot0, module parameters, EFS mount"),
    ("net", "addresses and routes, DHCP on port 68, wpa_supplicant via the compat runner"),
    ("firstboot", "the raw A/B node ULTAR-BOOTCTL opens for root only (the installer's answers record)"),
    ("netzregeln", "firewall rules"),
    ("ril", "modem IPC devices, address and routes of the mobile data interface"),
    ("seat", "owner and mode of root's device nodes below /dev (ultar-devd-seat, no bus)"),
    ("selftest", "root-only control socket, dpkg and programs in the compat tree"),
    ("shell", "emergency administrator shell at the console"),
    ("tontest", "root-only control socket (power off after the tone)"),
    ("updated", "writes the inactive A/B slot and the boot control (raw nodes open for root only)"),
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

/// The sound server is the only holder of the kernel's pcm node: every other program plays and
/// records through its stream socket (docs/integration/ultar-audiod.md).
#[test]
fn only_the_sound_server_keeps_the_sound_right() {
    let bit = RIGHT_NAMES.iter().position(|n| *n == "sound").unwrap();
    let holders: Vec<String> = shipped()
        .into_iter()
        .filter(|(_, _, spec)| spec.confinement.rights & (1 << bit) != 0)
        .map(|(_, _, spec)| spec.name)
        .filter(|name| !FULL_RIGHTS_ALLOWED.contains(&name.as_str()))
        .collect();
    assert_eq!(holders, ["audiod"], "who keeps the right sound");
}

#[test]
fn the_sound_server_knows_its_clients_by_user_id_and_shares_no_group_with_them() {
    let audiod = shipped()
        .into_iter()
        .find(|(_, _, spec)| spec.name == "audiod")
        .unwrap();
    let text = audiod.1;
    for client in ["calld:829", "mediad:824"] {
        assert!(text.contains(client), "no stream socket for {client}");
    }
    // a group of calld or mediad would also open their bus and log sockets (0660): the
    // sound server must not be a member of any client's group
    for client_group in [829, 824] {
        assert!(
            !audiod.2.confinement.groups.contains(&client_group),
            "audiod is in group {client_group}"
        );
    }
    assert!(
        !text.contains("app.browser"),
        "no user id for the browser yet"
    );
    assert!(text.contains("--device") && text.contains("/dev/ultar/pcm"));
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

/// `ids.toml`: users (own group of the same id), other groups, members.
struct Ids {
    users: Vec<(String, u32)>,
    groups: Vec<(String, u32)>,
    members: Vec<(String, Vec<String>)>,
}

fn ids() -> Ids {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/ids.toml");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let table: toml::Table = text.parse().unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let section = |name: &str| {
        table
            .get(name)
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default()
    };
    let numbers = |name: &str| -> Vec<(String, u32)> {
        section(name)
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    v.as_integer()
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or(u32::MAX),
                )
            })
            .collect()
    };
    let members = section("members")
        .into_iter()
        .map(|(k, v)| {
            let names = v.as_array().map(|a| {
                a.iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            });
            (k, names.unwrap_or_default())
        })
        .collect();
    Ids {
        users: numbers("users"),
        groups: numbers("groups"),
        members,
    }
}

#[test]
fn every_id_a_service_names_is_defined_once() {
    let ids = ids();
    let mut all: Vec<u32> = ids
        .users
        .iter()
        .chain(&ids.groups)
        .map(|(_, id)| *id)
        .collect();
    assert!(!all.contains(&u32::MAX), "an id is not a number");
    all.sort_unstable();
    let count = all.len();
    all.dedup();
    assert_eq!(all.len(), count, "an id is used twice in ids.toml");
    let user_ids: Vec<u32> = ids.users.iter().map(|(_, id)| *id).collect();
    for (path, _, spec) in shipped() {
        let c = &spec.confinement;
        if let Some(uid) = c.uid {
            assert!(
                user_ids.contains(&uid),
                "{path:?}: user {uid} not in ids.toml"
            );
        }
        for gid in c.gid.iter().chain(&c.groups) {
            assert!(all.contains(gid), "{path:?}: group {gid} not in ids.toml");
        }
    }
    let names: Vec<&String> = ids
        .users
        .iter()
        .chain(&ids.groups)
        .map(|(n, _)| n)
        .collect();
    for (group, members) in &ids.members {
        assert!(names.contains(&group), "members of unknown group {group}");
        for member in members {
            assert!(
                ids.users.iter().any(|(n, _)| n == member),
                "unknown member {member}"
            );
        }
    }
}

#[test]
fn the_seat_group_holds_the_compositor_and_only_root_sets_its_nodes() {
    let ids = ids();
    let seat = ids
        .groups
        .iter()
        .find(|(n, _)| n == "seat")
        .map(|(_, id)| *id);
    assert_eq!(seat, Some(812));
    let specs: Vec<ServiceSpec> = shipped().into_iter().map(|(_, _, spec)| spec).collect();
    let in_seat: Vec<&str> = specs
        .iter()
        .filter(|s| s.confinement.groups.contains(&812) || s.confinement.gid == Some(812))
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        in_seat,
        ["compositor"],
        "only the compositor may open the seat's nodes"
    );
    let members = ids
        .members
        .iter()
        .find(|(g, _)| g == "seat")
        .map(|(_, m)| m.clone());
    assert_eq!(members, Some(vec!["compositor".to_owned()]));
    let compositor = specs
        .iter()
        .find(|s| s.name == "compositor")
        .map(|s| s.program.args.clone());
    let args = compositor.unwrap_or_default();
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    assert_eq!(after("--seat-group").as_deref(), Some("812"));
    assert_eq!(
        after("--seat-state").as_deref(),
        Some("/run/ultar/seat/modes")
    );
    let seat_service = specs
        .iter()
        .find(|s| s.name == "seat")
        .map(|s| s.confinement.clone());
    let seat_service = seat_service.unwrap_or_default();
    assert_eq!(seat_service.uid, None);
    assert_eq!(seat_service.rights, 1 << 9, "file_admin only");
}
