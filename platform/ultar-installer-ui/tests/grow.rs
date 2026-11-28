//! Whole disk: the data partition takes the rest of the disk and the medium's ext4 grows into
//! it – plain and inside LUKS2. Checked with e2fsck; skipped without e2fsprogs.
mod common;
use common::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use ultar_installer_ui::engine::{write, BlockDevice, FileDevice, Plan};
use ultar_installer_ui::wizard::Step;
use ultar_slots::install::Fill;

/// A medium whose data partition holds a real 8 MiB ext4 (`None` without mke2fs).
fn ext4_medium(dir: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(dir.join("tree")).unwrap();
    std::fs::write(dir.join("tree/probe.txt"), b"ultar data probe\n").unwrap();
    let fs = dir.join("data.ext4");
    let made = Command::new("mke2fs")
        .args(["-q", "-F", "-t", "ext4", "-d"])
        .arg(dir.join("tree"))
        .arg(&fs)
        .arg("8M")
        .status();
    if !made.is_ok_and(|s| s.success()) {
        return None;
    }
    let path = dir.join("medium.img");
    let parts = [
        ("ULTAR-EFI", 2048, 4095),
        ("ultar-kernel", 4096, 6143),
        ("ultar-compat", 6144, 10239),
        ("ultar-data", 10240, 26623),
    ];
    gpt_image(&path, 16 * MIB, &parts);
    let image = std::fs::read(&fs).unwrap();
    let dev = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&dev, &image, 10240 * 512).unwrap();
    Some(path)
}

fn installed(src: &Path, tgt: &Path, pass: Option<&str>) -> Plan {
    use ultar_installer_ui::secret::Secret;
    use ultar_installer_ui::wizard::Encryption;
    let mut w = wizard_at(src, vec![disk_info(tgt, "Disk")], Step::Summary);
    if let Some(p) = pass {
        w.encryption = Encryption::Passphrase;
        w.secrets.passphrase = [Secret::new(p), Secret::new(p)];
        w.replan();
    }
    w.erase_confirmed = true;
    let (confirm, mut plan) = w.confirm_install().expect("confirm");
    plan.luks = ultar_luks::Params::TEST;
    write::execute(&plan, &confirm, &mut |_d: u64, _t: u64| {}).unwrap();
    plan
}

/// The data file system as an image file (decrypted when `pass` is given).
fn data_image(dir: &Path, tgt: &Path, plan: &Plan, pass: Option<&str>) -> PathBuf {
    let t = plan.layout.iter().find(|t| t.name == "ultar-data").unwrap();
    let mut dev = FileDevice::open_read(tgt).unwrap();
    let mut bytes = vec![0u8; (t.sectors * 512) as usize];
    dev.read_at(t.first * 512, &mut bytes).unwrap();
    if let Some(p) = pass {
        let opened = ultar_luks::open_bytes(&bytes, p.as_bytes()).expect("opens");
        bytes.drain(..opened.offset as usize);
        ultar_luks::Sectors::from_key(&opened.key).decrypt(0, &mut bytes);
    }
    let out = dir.join("data-out.img");
    std::fs::write(&out, &bytes).unwrap();
    out
}

/// e2fsck clean, grown to (nearly) the image's size, the medium's file still there.
fn assert_grown(name: &str, img: &Path) {
    let fsck = Command::new("e2fsck").arg("-fn").arg(img).output().unwrap();
    assert!(
        fsck.status.success(),
        "{name}: e2fsck\n{}",
        String::from_utf8_lossy(&fsck.stdout)
    );
    let dump = Command::new("dumpe2fs")
        .arg("-h")
        .arg(img)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&dump.stdout).into_owned();
    let blocks: u64 = text
        .lines()
        .find(|l| l.starts_with("Block count:"))
        .unwrap()
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    let size = std::fs::metadata(img).unwrap().len();
    assert!(
        blocks * 4096 > size - (8 << 20) || blocks * 1024 > size - (8 << 20),
        "{name}: grown to the partition ({blocks} blocks, {size} bytes)"
    );
    let cat = Command::new("debugfs")
        .args(["-R", "cat /probe.txt"])
        .arg(img)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&cat.stdout).contains("ultar data probe"),
        "{name}: data kept"
    );
}

fn check(name: &str, pass: Option<&str>) {
    let dir = scratch(name);
    let Some(src) = ext4_medium(&dir) else {
        eprintln!("note: mke2fs missing, {name} skipped");
        return;
    };
    let tgt = dir.join("target.img");
    std::fs::File::create(&tgt)
        .unwrap()
        .set_len(260 * MIB)
        .unwrap();
    let plan = installed(&src, &tgt, pass);
    let t = plan.layout.iter().find(|t| t.name == "ultar-data").unwrap();
    let Fill::Copy((f, l)) = t.fill else {
        panic!("data is copied")
    };
    assert!(
        t.sectors > 4 * (l - f + 1),
        "{name}: the data partition takes the rest of the disk"
    );
    let img = data_image(&dir, &tgt, &plan, pass);
    assert_grown(name, &img);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn plain_data_grows_to_the_rest_of_the_disk() {
    check("grow-plain", None);
}

#[test]
fn encrypted_data_grows_to_the_rest_of_the_disk() {
    check("grow-luks", Some("ein-langes-datenpasswort"));
}
