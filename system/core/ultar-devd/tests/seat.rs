//! `ultar-devd-seat` as a program: `--once` on a prepared tree, and as a
//! service following device events (hot-plug). Needs root and `mknod`; the
//! hot-plug part also needs permission to send a device event (root on Linux),
//! else it is skipped with a note.

use rustix::net::netlink::{self, SocketAddrNetlink};
use rustix::net::{sendto, socket_with, AddressFamily, SendFlags, SocketFlags, SocketType};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_ultar-devd-seat");

struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tree(tag: &str) -> Option<Tree> {
    let root = fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0);
    let dir = std::env::temp_dir().join(format!("seat-bin-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for sub in ["dev/input", "sys/class/input", "state"] {
        fs::create_dir_all(dir.join(sub)).ok()?;
    }
    for sub in ["", "dev", "dev/input", "state"] {
        fs::set_permissions(dir.join(sub), fs::Permissions::from_mode(0o755)).ok()?;
    }
    fs::write(dir.join("seat.toml"), "group = 812\n").ok()?;
    let tree = Tree(dir);
    (root && node(&tree.0, "uinput", "c")).then_some(tree)
}

fn node(root: &Path, devname: &str, _: &str) -> bool {
    Command::new("mknod")
        .args(["-m", "666"])
        .arg(root.join("dev").join(devname))
        .args(["c", "1", "3"])
        .status()
        .is_ok_and(|s| s.success())
}

fn hardware(root: &Path, event: &str) {
    let target = root
        .join("sys/devices/pci0000:00/usb1/input/input1")
        .join(event);
    fs::create_dir_all(target.join("device")).unwrap();
    fs::write(target.join("device/name"), "keyboard\n").unwrap();
    std::os::unix::fs::symlink(&target, root.join("sys/class/input").join(event)).unwrap();
}

fn look(root: &Path, devname: &str) -> (u32, u32, u32) {
    let m = fs::symlink_metadata(root.join("dev").join(devname)).unwrap();
    (m.uid(), m.gid(), m.mode() & 0o7777)
}

fn args(root: &Path) -> Vec<String> {
    let path = |p: &str| root.join(p).to_string_lossy().into_owned();
    vec![
        "--config".into(),
        path("seat.toml"),
        "--root".into(),
        path(""),
        "--state".into(),
        path("state"),
        "--lang".into(),
        "en".into(),
    ]
}

#[test]
fn once_sets_the_nodes_and_writes_the_state() {
    let Some(tree) = tree("once") else {
        eprintln!("skipped: needs root and mknod");
        return;
    };
    hardware(&tree.0, "event0");
    assert!(node(&tree.0, "input/event0", "c"));
    let out = Command::new(BIN)
        .args(args(&tree.0))
        .arg("--once")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("input/event0 seat-input 0:812 0660 set"),
        "{text}"
    );
    assert!(text.contains("uinput uinput 0:0 0600 set"), "{text}");
    assert!(text.contains("device modes: kept"), "{text}");
    assert_eq!(look(&tree.0, "input/event0"), (0, 812, 0o660));
    assert_eq!(look(&tree.0, "uinput"), (0, 0, 0o600));
    assert_eq!(
        fs::read_to_string(tree.0.join("state/modes")).unwrap(),
        "kept\n"
    );
    let bad = Command::new(BIN).args(["--once"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

/// Sends a device event the way the kernel does (multicast group 1).
fn announce(devname: &str) -> bool {
    let flags = SocketFlags::CLOEXEC;
    let Ok(fd) = socket_with(
        AddressFamily::NETLINK,
        SocketType::DGRAM,
        flags,
        Some(netlink::KOBJECT_UEVENT),
    ) else {
        return false;
    };
    let message = format!(
        "add@/devices/test/{devname}\0ACTION=add\0SUBSYSTEM=input\0DEVNAME={devname}\0SEQNUM=1\0"
    );
    sendto(
        &fd,
        message.as_bytes(),
        SendFlags::empty(),
        &SocketAddrNetlink::new(0, 1),
    )
    .is_ok()
}

fn until(limit: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn the_service_handles_a_device_added_later() {
    let Some(tree) = tree("hotplug") else {
        eprintln!("skipped: needs root and mknod");
        return;
    };
    let mut child = Command::new(BIN)
        .args(args(&tree.0))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let state = tree.0.join("state/modes");
    assert!(
        until(Duration::from_secs(10), || state.exists()),
        "no state file"
    );
    assert_eq!(look(&tree.0, "uinput"), (0, 0, 0o600));
    hardware(&tree.0, "event4");
    assert!(node(&tree.0, "input/event4", "c"));
    let sent = announce("input/event4");
    let seen = sent
        && until(Duration::from_secs(10), || {
            look(&tree.0, "input/event4") == (0, 812, 0o660)
        });
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let status = child.wait().unwrap();
    if !sent {
        eprintln!("hot-plug part skipped: sending a device event is not permitted here");
        return;
    }
    assert!(
        seen,
        "the node added later was not set: {:?}",
        look(&tree.0, "input/event4")
    );
    assert!(status.success(), "{status:?}");
}
