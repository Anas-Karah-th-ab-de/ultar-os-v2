//! Test support: installation media and target disks as image files (and loop devices),
//! the board's devices, app construction and input.
#![allow(dead_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use ultar_bus::messages::device_service::{DeviceInfo, DevicePhase, DriverInfo};
use ultar_bus::{Text, TextArg};
use ultar_i18n::Lang;
use ultar_installer_ui::engine::{inspect, DiskInfo};
use ultar_installer_ui::ports::fake::{Fake, Shared, World};
use ultar_installer_ui::wizard::{Step, Wizard};
use ultar_installer_ui::InstallerApp;
use ultar_slots::gpt::{build_entry, build_header, Header};
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::{Point, Size};

pub const MIB: u64 = 1 << 20;

pub fn scratch(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("ultar-installer-{}-{n}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// Writes a GPT with `parts` (name, first sector, last sector) into an image of `bytes`.
pub fn gpt_image(path: &Path, bytes: u64, parts: &[(&str, u64, u64)]) {
    let f = std::fs::File::create(path).expect("image");
    f.set_len(bytes).expect("size");
    drop(f);
    let n = bytes / 512;
    let mut entries = vec![0u8; 128 * 128];
    for (i, (name, first, last)) in parts.iter().enumerate() {
        build_entry(
            &mut entries[i * 128..],
            [0xAF; 16],
            [i as u8 + 1; 16],
            *first,
            *last,
            name,
        );
    }
    let h = Header {
        my_lba: 1,
        alternate_lba: n - 1,
        first_usable: 34,
        last_usable: n - 34,
        entries_lba: 2,
        entry_count: 128,
        entry_size: 128,
        entries_crc: ultar_slots::crc32(&entries),
        disk_guid: [9; 16],
    };
    let mut img = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open");
    use std::io::{Seek, SeekFrom, Write};
    img.seek(SeekFrom::Start(512)).expect("seek");
    img.write_all(&build_header(&h, &entries)).expect("hdr");
    img.write_all(&entries).expect("entries");
}

/// An installation medium: four partitions, each filled with its own byte pattern.
pub fn medium(dir: &Path) -> PathBuf {
    let path = dir.join("medium.img");
    let parts = [
        ("ULTAR-EFI", 2048, 4095),
        ("ultar-kernel", 4096, 6143),
        ("ultar-compat", 6144, 10239),
        ("ultar-data", 10240, 12287),
    ];
    gpt_image(&path, 8 * MIB, &parts);
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open");
    for (i, (_, first, last)) in parts.iter().enumerate() {
        f.seek(SeekFrom::Start(first * 512)).expect("seek");
        let len = ((last - first + 1) * 512) as usize;
        let data: Vec<u8> = (0..len)
            .map(|b| (b as u8).wrapping_mul(31).wrapping_add(i as u8 * 17 + 1))
            .collect();
        f.write_all(&data).expect("fill");
    }
    path
}

/// A target disk image with an existing system ("Windows 11").
pub fn target(dir: &Path, bytes: u64) -> PathBuf {
    let path = dir.join("target.img");
    gpt_image(
        &path,
        bytes,
        &[
            ("EFI system partition", 2048, 4095),
            ("Windows 11", 4096, 8191),
        ],
    );
    path
}

/// A loop device for `image` (`None` without permission: tests then use the image file).
pub struct Loop(pub PathBuf);

impl Loop {
    pub fn attach(image: &Path) -> Option<Loop> {
        let out = std::process::Command::new("losetup")
            .args(["-f", "--show"])
            .arg(image)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let dev = String::from_utf8(out.stdout).ok()?.trim().to_owned();
        (!dev.is_empty()).then(|| Loop(PathBuf::from(dev)))
    }
}

impl Drop for Loop {
    fn drop(&mut self) {
        let _ = std::process::Command::new("losetup")
            .arg("-d")
            .arg(&self.0)
            .status();
    }
}

pub fn sha(path: &Path) -> Vec<u8> {
    let data = std::fs::read(path).expect("read");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut out = Vec::new();
    for chunk in data.chunks(1 << 16) {
        for b in chunk {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        out.extend_from_slice(&h.to_le_bytes());
    }
    out
}

fn driver(id: &str, class: &str) -> DriverInfo {
    DriverInfo {
        id: id.into(),
        version: "1.0.0".into(),
        class: class.into(),
        name_key: format!("driver.{id}"),
        quarantined: None,
    }
}

pub fn devices() -> Vec<DeviceInfo> {
    let dev = |n: u8, label: &str, id: &str, class: &str, reason: Option<Text>| DeviceInfo {
        address: format!("pci:00:{n:02x}.0"),
        hardware_id: format!("pci:8086:{n:04x}"),
        label: Some(label.into()),
        phase: DevicePhase::Active,
        active: Some(driver(id, class)),
        alternatives: vec![driver(id, class), driver("ultar-generic", class)],
        previous: None,
        capabilities: vec![class.into()],
        reason,
    };
    let mut l = vec![
        dev(
            2,
            "Intel Arc (مدمجة)",
            "ultar-gfx-intel",
            "gpu",
            Some(Text::key("installer.note.full_accel")),
        ),
        dev(20, "Intel AX211", "ultar-wifi-iwl", "wifi", None),
        dev(31, "Realtek RTL8125", "ultar-net-r8125", "net", None),
        dev(14, "Realtek ALC295", "ultar-audio-hda", "audio_out", None),
        dev(21, "Intel AX211", "ultar-bt-intel", "bluetooth", None),
        dev(25, "ELAN I2C", "ultar-input-i2c", "input", None),
        dev(
            26,
            "USB UVC",
            "ultar-cam-uvc",
            "camera",
            Some(Text::key("installer.limited.max_res").with(TextArg::Verbatim("720p".into()))),
        ),
    ];
    l.push(DeviceInfo {
        address: "usb:1-7".into(),
        hardware_id: "usb:27c6:5395".into(),
        label: Some("Goodix 27C6".into()),
        phase: DevicePhase::NoDriver,
        active: None,
        alternatives: vec![],
        previous: None,
        capabilities: vec!["fingerprint".into()],
        reason: Some(Text::key("installer.note.no_fp_login")),
    });
    l
}

pub fn world() -> Shared {
    Rc::new(RefCell::new(World {
        devices: Some(devices()),
        ..World::default()
    }))
}

pub fn app_with(
    world: &Shared,
    wizard: Wizard,
    lang: Lang,
    theme: ThemeKind,
    size: Size,
) -> InstallerApp {
    let tr = ultar_installer_ui::i18n::translator(lang);
    let ui = Ui::new(PaintEnv::new(lang, theme), InputMode::Pointer, size, tr).expect("ui");
    InstallerApp::new(
        ui,
        wizard,
        Box::new(Fake(world.clone())),
        Box::new(Fake(world.clone())),
    )
    .expect("app")
}

/// A wizard on `step` with everything before it filled in (as on the boards).
pub fn wizard_at(source: &Path, disks: Vec<DiskInfo>, step: Step) -> Wizard {
    let mut w = Wizard::new(source.to_path_buf(), disks);
    w.account = [
        "أنس".into(),
        "anas".into(),
        "ultar-pc".into(),
        String::new(),
    ];
    w.reached = step;
    w.step = step;
    if step >= Step::Disk && !w.disks.is_empty() {
        w.choose_disk(0);
    }
    w
}

pub fn disk_info(path: &Path, name: &str) -> DiskInfo {
    inspect(path, name).expect("inspect")
}

pub fn center(app: &mut InstallerApp, id: NodeId) -> Point {
    app.ui.layout();
    app.ui.rect(id).expect("rect").center()
}

pub fn click(app: &mut InstallerApp, id: NodeId) {
    let pos = center(app, id);
    app.handle(Event::PointerMove { pos }).expect("move");
    app.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
    .expect("down");
    app.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    })
    .expect("up");
}

pub fn write_png(path: &Path, ui: &mut Ui) {
    let canvas = ultar_ui::snapshot::render(ui).expect("render");
    let image = ultar_ui::snapshot::Image::from_canvas(&canvas);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(path, image.encode_png().expect("png")).expect("write");
}

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
