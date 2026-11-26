use super::apply::{seat_nodes, Applied, DeviceModes};
use super::*;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

const SEAT: u32 = 812;

fn config(allowed: &[&str]) -> SeatConfig {
    SeatConfig {
        group: SEAT,
        torch_group: None,
        virtual_allowed: allowed.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// A prepared tree: `/sys/class/input/<event>` links to a hardware or a
/// software (`/devices/virtual/`) device with a name.
struct Tree(PathBuf);

impl Tree {
    fn new(tag: &str) -> Tree {
        let dir = std::env::temp_dir().join(format!("devd-seat-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["dev/input", "dev/dri", "sys/class/input"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        for sub in ["", "dev", "dev/input", "dev/dri"] {
            fs::set_permissions(dir.join(sub), fs::Permissions::from_mode(0o755)).unwrap();
        }
        Tree(dir)
    }

    fn input(&self, event: &str, device: &str, name: &str) {
        let target = self.0.join("sys").join(device).join(event);
        fs::create_dir_all(target.join("device")).unwrap();
        fs::write(target.join("device/name"), format!("{name}\n")).unwrap();
        std::os::unix::fs::symlink(&target, self.0.join("sys/class/input").join(event)).unwrap();
    }

    /// A character node (`mknod`, root only); false where that is not allowed.
    fn node(&self, devname: &str, mode: u32) -> bool {
        let path = self.0.join("dev").join(devname);
        let made = std::process::Command::new("mknod")
            .args(["-m", &format!("{mode:o}")])
            .arg(&path)
            .args(["c", "1", "3"])
            .status()
            .is_ok_and(|s| s.success());
        made && fs::symlink_metadata(&path).is_ok()
    }

    fn look(&self, devname: &str) -> (u32, u32, u32) {
        let m = fs::symlink_metadata(self.0.join("dev").join(devname)).unwrap();
        (m.uid(), m.gid(), m.mode() & 0o7777)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn is_root() -> bool {
    fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

#[test]
fn the_configuration_is_checked() {
    let c = SeatConfig::parse("group = 812\nvirtual_allowed = [\"ultar-osk\"]\n").unwrap();
    assert_eq!(c, config(&["ultar-osk"]));
    assert_eq!(
        SeatConfig::parse("group = 812\n").unwrap().virtual_allowed,
        Vec::<String>::new()
    );
    assert_eq!(
        SeatConfig::parse("group = 0\n"),
        Err(ConfigError::Bound("group"))
    );
    assert!(matches!(
        SeatConfig::parse("group = 812\nother = 1\n"),
        Err(ConfigError::Syntax(_))
    ));
    assert!(matches!(
        SeatConfig::parse("group = -1\n"),
        Err(ConfigError::Syntax(_))
    ));
    let many = format!(
        "group = 812\nvirtual_allowed = [{}]\n",
        vec!["\"a\""; 17].join(",")
    );
    assert_eq!(
        SeatConfig::parse(&many),
        Err(ConfigError::Bound("virtual_allowed"))
    );
    assert!(SeatConfig::parse("group = 812\nvirtual_allowed = [\" \"]\n").is_err());
    assert!(SeatConfig::parse("group = 812\nvirtual_allowed = [\"a\\nb\"]\n").is_err());
    let shipped =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/devd-seat.toml");
    let shipped = SeatConfig::load(&shipped).unwrap();
    assert_eq!((shipped.group, shipped.virtual_allowed.len()), (SEAT, 0));
}

#[test]
fn only_seat_nodes_are_looked_after() {
    for yes in [
        "uinput",
        "input/event0",
        "input/event12",
        "dri/card1",
        "fb0",
        "ion",
    ] {
        assert!(is_seat_node(yes), "{yes}");
    }
    for no in [
        "input/mice",
        "input/event",
        "input/event1a",
        "input/../uinput",
        "dri/renderD128",
        "fb",
        "sda",
        "input/event0/x",
        "/dev/fb0",
        "dri/card12345",
        "ion0",
    ] {
        assert!(!is_seat_node(no), "{no}");
    }
}

#[test]
fn hardware_joins_the_seat_software_only_when_allowed() {
    let tree = Tree::new("classify");
    tree.input(
        "event0",
        "devices/platform/i8042/serio0/input/input1",
        "AT keyboard",
    );
    tree.input("event1", "devices/virtual/input/input7", "ultar-osk");
    tree.input("event2", "devices/virtual/input/input8", "injector");
    let c = config(&["ultar-osk"]);
    let reason = |name: &str| wanted(&tree.0, name, &c).map(|w| (w.reason, w.gid, w.mode));
    assert_eq!(
        reason("input/event0"),
        Some((Reason::SeatInput, SEAT, 0o660))
    );
    assert_eq!(
        reason("input/event1"),
        Some((Reason::AllowedVirtual, SEAT, 0o660))
    );
    assert_eq!(
        reason("input/event2"),
        Some((Reason::RefusedVirtual, 0, 0o600))
    );
    assert_eq!(reason("input/event9"), Some((Reason::Untraced, 0, 0o600)));
    assert_eq!(reason("uinput"), Some((Reason::Uinput, 0, 0o600)));
    assert_eq!(
        reason("dri/card0"),
        Some((Reason::SeatDisplay, SEAT, 0o660))
    );
    assert_eq!(reason("fb0"), Some((Reason::SeatDisplay, SEAT, 0o660)));
    assert_eq!(reason("input/mice"), None);
    let none = config(&[]);
    assert_eq!(
        wanted(&tree.0, "input/event1", &none).map(|w| w.reason),
        Some(Reason::RefusedVirtual)
    );
}

#[test]
fn nodes_get_owner_group_and_mode_and_uinput_stays_root_only() {
    let tree = Tree::new("apply");
    if !is_root() || !tree.node("input/event0", 0o600) {
        eprintln!("skipped: needs root and mknod");
        return;
    }
    tree.input(
        "event0",
        "devices/pci0000:00/usb1/input/input3",
        "USB keyboard",
    );
    tree.input("event1", "devices/virtual/input/input9", "injector");
    assert!(tree.node("input/event1", 0o666) && tree.node("uinput", 0o666));
    assert!(tree.node("dri/card0", 0o600) && tree.node("fb0", 0o644));
    fs::write(tree.0.join("dev/input/mice"), b"").unwrap();
    assert_eq!(
        seat_nodes(&tree.0),
        ["dri/card0", "fb0", "input/event0", "input/event1", "uinput"]
    );
    let reports = scan(&tree.0, &config(&[]));
    assert!(
        reports.iter().all(|r| r.applied == Applied::Set),
        "{reports:?}"
    );
    assert_eq!(tree.look("input/event0"), (0, SEAT, 0o660));
    assert_eq!(
        tree.look("input/event1"),
        (0, 0, 0o600),
        "software device refused"
    );
    assert_eq!(tree.look("uinput"), (0, 0, 0o600));
    assert_eq!(tree.look("dri/card0"), (0, SEAT, 0o660));
    assert_eq!(tree.look("fb0"), (0, SEAT, 0o660));
    assert_eq!(DeviceModes::of(&reports, false), DeviceModes::Kept);
    let again = scan(&tree.0, &config(&[]));
    assert!(again.iter().all(|r| r.applied == Applied::Unchanged));
}

#[test]
fn links_and_directories_others_may_change_are_left_alone() {
    let tree = Tree::new("links");
    if !is_root() || !tree.node("fb0", 0o600) {
        eprintln!("skipped: needs root and mknod");
        return;
    }
    std::os::unix::fs::symlink(tree.0.join("dev/fb0"), tree.0.join("dev/input/event3")).unwrap();
    tree.input("event3", "devices/pci0000:00/usb1/input/input4", "mouse");
    let w = wanted(&tree.0, "input/event3", &config(&[])).unwrap();
    assert_eq!(
        apply_node(&tree.0, "input/event3", w).applied,
        Applied::NotDevice
    );
    assert_eq!(
        tree.look("fb0"),
        (0, 0, 0o600),
        "the link target is untouched"
    );
    fs::set_permissions(tree.0.join("dev"), fs::Permissions::from_mode(0o777)).unwrap();
    let w = wanted(&tree.0, "fb0", &config(&[])).unwrap();
    assert_eq!(apply_node(&tree.0, "fb0", w).applied, Applied::NotDevice);
    assert_eq!(tree.look("fb0"), (0, 0, 0o600));
}

#[test]
fn the_verdict_reads_a_refusal_as_kernel_gating_only_on_the_ultar_kernel() {
    let w = Wanted {
        uid: 0,
        gid: SEAT,
        mode: 0o660,
        reason: Reason::SeatInput,
    };
    let r = |applied| NodeReport {
        devname: "input/event0".into(),
        wanted: w,
        applied,
    };
    let refused = [r(Applied::Set), r(Applied::KernelRefused)];
    assert_eq!(DeviceModes::of(&refused, true), DeviceModes::KernelGated);
    assert_eq!(DeviceModes::of(&refused, false), DeviceModes::Failed);
    assert_eq!(
        DeviceModes::of(&[r(Applied::Failed("x".into()))], true),
        DeviceModes::Failed
    );
    assert_eq!(
        DeviceModes::of(&[r(Applied::NotDevice), r(Applied::Unchanged)], false),
        DeviceModes::Kept
    );
    assert_eq!(DeviceModes::of(&[], true), DeviceModes::Kept);
}

#[test]
fn the_state_file_holds_one_word() {
    let tree = Tree::new("state");
    let dir = tree.0.join("state");
    fs::create_dir(&dir).unwrap();
    write_state(&dir, DeviceModes::KernelGated).unwrap();
    write_state(&dir, DeviceModes::Kept).unwrap();
    let file = dir.join(STATE_FILE);
    assert_eq!(fs::read_to_string(&file).unwrap(), "kept\n");
    assert_eq!(fs::metadata(&file).unwrap().mode() & 0o777, 0o644);
    assert!(write_state(&tree.0.join("missing"), DeviceModes::Kept).is_err());
    std::os::unix::fs::symlink(&dir, tree.0.join("link")).unwrap();
    assert!(write_state(&tree.0.join("link"), DeviceModes::Kept).is_err());
}

#[test]
fn device_events_are_parsed() {
    let add = DeviceEvent::parse(b"add@/devices/virtual/input/input9/event5\0ACTION=add\0SUBSYSTEM=input\0DEVNAME=input/event5\0SEQNUM=7\0").unwrap();
    assert_eq!(
        (add.action.as_str(), add.devname.as_str()),
        ("add", "input/event5")
    );
    assert!(add.wants_apply());
    let abs = DeviceEvent::parse(b"change@/x\0DEVNAME=/dev/dri/card0\0").unwrap();
    assert_eq!(abs.devname, "dri/card0");
    assert!(abs.wants_apply());
    assert!(!DeviceEvent::parse(b"remove@/x\0DEVNAME=input/event5\0")
        .unwrap()
        .wants_apply());
    assert!(!DeviceEvent::parse(b"add@/x\0DEVNAME=sda\0")
        .unwrap()
        .wants_apply());
    assert!(!DeviceEvent::parse(b"add@/x\0DEVNAME=input/../../etc/x\0")
        .unwrap()
        .wants_apply());
    assert!(DeviceEvent::parse(b"libudev\0\xfe\xed").is_none());
    assert!(DeviceEvent::parse(b"").is_none());
    assert!(!DeviceEvent::parse(b"add@/x\0DEVNAME=\xff\0")
        .unwrap()
        .wants_apply());
}

/// Hostile event messages never panic and never name a node outside the seat
/// set (fixed seed).
#[test]
fn random_event_messages_are_harmless() {
    let mut state = 0x5eed_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };
    let pieces: [&[u8]; 6] = [
        b"add@",
        b"DEVNAME=",
        b"input/event",
        b"\0",
        b"../",
        b"uinput",
    ];
    for _ in 0..20_000 {
        let mut message = Vec::new();
        for _ in 0..(next() % 12) {
            match next() % 3 {
                0 => message.extend_from_slice(pieces[usize::from(next()) % pieces.len()]),
                1 => message.push(next()),
                _ => message.push(b'0' + next() % 10),
            }
        }
        if let Some(event) = DeviceEvent::parse(&message) {
            if event.wants_apply() {
                assert!(is_seat_node(&event.devname) && !event.devname.contains(".."));
            }
        }
    }
}
