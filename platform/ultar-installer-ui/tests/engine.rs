//! The writing engine against image files and loop devices.
mod common;
use common::*;
use std::path::Path;
use ultar_installer_ui::engine::answers::{self, Answers};
use ultar_installer_ui::engine::{
    partition_names, read_gpt, write, BlockDevice, EngineError, FileDevice, Plan,
};
use ultar_installer_ui::wizard::{Step, Wizard};
use ultar_slots::install::{Fill, PARTS};
use ultar_slots::SlotTable;

fn confirmed(
    source: &Path,
    target: &Path,
) -> (ultar_installer_ui::engine::EraseConfirmation, Plan) {
    let mut w = wizard_at(source, vec![disk_info(target, "Disk")], Step::Summary);
    w.erase_confirmed = true;
    w.confirm_install().expect("confirm")
}

fn check_installed(source: &Path, target: &Path, plan: &Plan) {
    let mut dst = FileDevice::open_read(target).unwrap();
    let mut src = FileDevice::open_read(source).unwrap();
    let (h, entries) = read_gpt(&mut dst).expect("gpt after install");
    let names = partition_names(&h, &entries);
    assert_eq!(
        names,
        [
            "ULTAR-EFI",
            "ULTAR-BOOTCTL",
            "ultar-kernel-a",
            "ultar-kernel-b",
            "ultar-system-a",
            "ultar-system-b",
            "ultar-data"
        ]
    );
    for t in plan.layout.iter().filter(|t| t.sectors > 0) {
        if let Fill::Copy((f, l)) = t.fill {
            let n = ((l - f + 1) * 512) as usize;
            let (mut a, mut b) = (vec![0u8; n], vec![0u8; n]);
            src.read_at(f * 512, &mut a).unwrap();
            dst.read_at(t.first * 512, &mut b).unwrap();
            assert!(a == b, "{} copied", t.name);
        }
    }
    let boot = plan.layout[1];
    let mut table = [0u8; 64];
    dst.read_at(boot.first * 512, &mut table).unwrap();
    assert_eq!(SlotTable::decode(&table).unwrap(), SlotTable::factory());
    let mut record = vec![0u8; answers::MAX_BYTES];
    dst.read_at(boot.first * 512 + answers::OFFSET as u64, &mut record)
        .unwrap();
    let a = Answers::decode(&record).unwrap();
    assert_eq!(a.get("user"), Some("anas"));
    assert_eq!(PARTS, 8);
}

#[test]
fn installs_onto_an_image_file() {
    let dir = scratch("image");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let (confirm, plan) = confirmed(&src, &tgt);
    let mut steps = 0;
    write::execute(&plan, &confirm, &mut |_d: u64, _t: u64| steps += 1).unwrap();
    assert!(steps > 0);
    check_installed(&src, &tgt, &plan);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn installs_encrypted_data_as_luks2() {
    use ultar_installer_ui::secret::Secret;
    use ultar_installer_ui::wizard::Encryption;
    let dir = scratch("luks");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let mut w = wizard_at(&src, vec![disk_info(&tgt, "Disk")], Step::Summary);
    let pass = "ein-langes-datenpasswort";
    w.encryption = Encryption::Passphrase;
    w.secrets.passphrase = [Secret::new(pass), Secret::new(pass)];
    w.replan();
    w.erase_confirmed = true;
    let (confirm, mut plan) = w.confirm_install().expect("confirm");
    plan.luks = ultar_luks::Params::TEST;
    write::execute(&plan, &confirm, &mut |_d: u64, _t: u64| {}).unwrap();
    let data = plan.layout.iter().find(|t| t.name == "ultar-data").unwrap();
    let Fill::Copy((f, l)) = data.fill else {
        panic!("data is copied")
    };
    assert!(data.sectors >= (l - f + 1) + ultar_installer_ui::engine::LUKS_RESERVE);
    let mut dst = FileDevice::open_read(&tgt).unwrap();
    let mut meta = vec![0u8; ultar_luks::DATA_OFFSET as usize];
    dst.read_at(data.first * 512, &mut meta).unwrap();
    assert!(ultar_luks::open_bytes(&meta, b"falsch").is_err());
    let opened = ultar_luks::open_bytes(&meta, pass.as_bytes()).expect("opens with the passphrase");
    let n = ((l - f + 1) * 512) as usize;
    let (mut plain, mut sealed) = (vec![0u8; n], vec![0u8; n]);
    FileDevice::open_read(&src)
        .unwrap()
        .read_at(f * 512, &mut plain)
        .unwrap();
    dst.read_at(data.first * 512 + opened.offset, &mut sealed)
        .unwrap();
    assert!(sealed != plain, "data on the disk is encrypted");
    ultar_luks::Sectors::from_key(&opened.key).decrypt(0, &mut sealed);
    assert!(sealed == plain, "decrypts to the medium's data");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn installs_onto_a_loop_device() {
    let dir = scratch("loop");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let Some(dev) = Loop::attach(&tgt) else {
        eprintln!("note: no loop device available, covered by the image-file test");
        std::fs::remove_dir_all(dir).unwrap();
        return;
    };
    let (confirm, plan) = confirmed(&src, &dev.0);
    write::execute(&plan, &confirm, &mut |_d: u64, _t: u64| {}).unwrap();
    check_installed(&src, &dev.0, &plan);
    drop(dev);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn refuses_small_disks_the_medium_itself_and_changed_targets() {
    let dir = scratch("refuse");
    let src = medium(&dir);
    let small = target(&dir, 64 * MIB);
    let w = wizard_at(&src, vec![disk_info(&small, "Small")], Step::Disk);
    assert!(matches!(w.plan, Some(Err(EngineError::TooSmall(..)))));
    assert_eq!(w.blocked(), Some("installer.reason.space"));
    let w = wizard_at(&src, vec![disk_info(&src, "Medium")], Step::Disk);
    assert_eq!(w.plan, Some(Err(EngineError::TargetIsSource)));
    let tgt = dir.join("big.img");
    gpt_image(&tgt, 300 * MIB, &[]);
    let (confirm, plan) = confirmed(&src, &tgt);
    let before = sha(&tgt);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&tgt)
        .unwrap()
        .set_len(310 * MIB)
        .unwrap();
    let err = write::execute(&plan, &confirm, &mut |_d: u64, _t: u64| {}).unwrap_err();
    assert_eq!(err, EngineError::TargetChanged);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&tgt)
        .unwrap()
        .set_len(300 * MIB)
        .unwrap();
    assert_eq!(sha(&tgt), before, "nothing written after a refusal");
    let other = dir.join("other.img");
    gpt_image(&other, 300 * MIB, &[]);
    let mut wrong = plan.clone();
    wrong.target = other.clone();
    let before = sha(&other);
    assert_eq!(
        write::execute(&wrong, &confirm, &mut |_d: u64, _t: u64| {}).unwrap_err(),
        EngineError::TargetChanged
    );
    assert_eq!(
        sha(&other),
        before,
        "a confirmation for another disk writes nothing"
    );
    let _ = Wizard::default();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_medium_without_source_partitions_is_refused() {
    let dir = scratch("nosrc");
    let bad = dir.join("bad.img");
    gpt_image(&bad, 8 * MIB, &[("ULTAR-EFI", 2048, 4095)]);
    let tgt = target(&dir, 300 * MIB);
    let w = wizard_at(&bad, vec![disk_info(&tgt, "Disk")], Step::Disk);
    assert_eq!(w.plan, Some(Err(EngineError::NoSource("ultar-kernel"))));
    let junk = dir.join("junk.img");
    std::fs::write(&junk, vec![0u8; 4096]).unwrap();
    let w = wizard_at(&junk, vec![disk_info(&tgt, "Disk")], Step::Disk);
    assert_eq!(w.plan, Some(Err(EngineError::NoSource("GPT"))));
    std::fs::remove_dir_all(dir).unwrap();
}

/// A target with a foreign system (64 MiB "Windows" partition, patterned) and free space.
fn foreign_target(dir: &Path, bytes: u64, extra: &[(&str, u64, u64)]) -> std::path::PathBuf {
    let path = dir.join("foreign.img");
    let mib = 2048u64;
    let mut parts = vec![("Windows", mib, 65 * mib - 1)];
    parts.extend_from_slice(extra);
    gpt_image(&path, bytes, &parts);
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    f.seek(SeekFrom::Start(mib * 512)).unwrap();
    f.write_all(&vec![0x5Au8; 64 << 20]).unwrap();
    path
}

fn range_sha(path: &Path, first: u64, last: u64) -> Vec<u8> {
    let mut f = FileDevice::open_read(path).unwrap();
    let mut buf = vec![0u8; ((last - first + 1) * 512) as usize];
    f.read_at(first * 512, &mut buf).unwrap();
    buf
}

#[test]
fn beside_installs_into_free_space_and_keeps_the_other_system() {
    let dir = scratch("beside");
    let src = medium(&dir);
    let tgt = foreign_target(&dir, 600 * MIB, &[]);
    let before = range_sha(&tgt, 2048, 65 * 2048 - 1);
    let old_entry = disk_info(&tgt, "Disk").table.unwrap()[0].entry;
    let mut w = wizard_at(&src, vec![disk_info(&tgt, "Disk")], Step::Disk);
    w.layout.mode = ultar_installer_ui::wizard::DiskMode::Beside;
    w.layout.share = 100;
    w.replan();
    assert!(w.blocked().is_none(), "{:?}", w.plan);
    w.step = Step::Summary;
    w.secrets.password = [
        ultar_installer_ui::secret::Secret::new("km-secret-1"),
        ultar_installer_ui::secret::Secret::new("km-secret-1"),
    ];
    w.erase_confirmed = true;
    let (confirm, plan) = w.confirm_install().expect("confirm");
    write::execute(&plan, &confirm, &mut |_, _| {}).expect("install");
    assert_eq!(
        range_sha(&tgt, 2048, 65 * 2048 - 1),
        before,
        "Windows untouched"
    );
    let info = disk_info(&tgt, "Disk");
    let table = info.table.unwrap();
    assert_eq!(table[0].entry, old_entry, "its entry kept bit for bit");
    for name in [
        "ULTAR-EFI",
        "ULTAR-BOOTCTL",
        "ultar-kernel-a",
        "ultar-system-b",
        "ultar-data",
    ] {
        assert!(table.iter().any(|e| e.name == name), "{name}");
    }
    for e in &table[1..] {
        assert!(e.first >= 65 * 2048, "{} after Windows", e.name);
    }
    let mut dst = FileDevice::open_read(&tgt).unwrap();
    let (h, entries) = read_gpt(&mut dst).unwrap();
    assert_eq!(h.disk_guid, [9; 16], "disk GUID kept");
    let boot = table.iter().find(|e| e.name == "ULTAR-BOOTCTL").unwrap();
    let mut record = vec![0u8; answers::MAX_BYTES];
    dst.read_at(boot.first * 512 + answers::OFFSET as u64, &mut record)
        .unwrap();
    let a = Answers::decode(&record).unwrap();
    assert_eq!(a.get("disk_mode"), Some("beside"));
    assert!(a.get("password_hash").unwrap().starts_with("$argon2id$"));
    assert!(
        !String::from_utf8_lossy(&record).contains("km-secret-1"),
        "no password"
    );
    drop(entries);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn beside_says_why_it_cannot() {
    let dir = scratch("beside-no");
    let src = medium(&dir);
    // no partition table at all
    let bare = dir.join("bare.img");
    std::fs::File::create(&bare)
        .unwrap()
        .set_len(600 * MIB)
        .unwrap();
    let mut w = wizard_at(&src, vec![disk_info(&bare, "Disk")], Step::Disk);
    w.layout.mode = ultar_installer_ui::wizard::DiskMode::Beside;
    w.replan();
    assert_eq!(w.plan, Some(Err(EngineError::NoTable)));
    assert_eq!(w.blocked(), Some("installer.reason.no_table"));
    // a table, but the free space is too small
    let full = foreign_target(&dir, 120 * MIB, &[]);
    let mut w = wizard_at(&src, vec![disk_info(&full, "Disk")], Step::Disk);
    w.layout.mode = ultar_installer_ui::wizard::DiskMode::Beside;
    w.replan();
    assert!(matches!(w.plan, Some(Err(EngineError::NoFreeSpace(need, have))) if need > have));
    assert_eq!(w.blocked(), Some("installer.reason.no_free_space"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Creates one new partition per role, each a little larger than the role needs.
fn assign_all(w: &mut ultar_installer_ui::wizard::Wizard, src: &Path) {
    use ultar_installer_ui::engine::{PartRole, Plan as P};
    let mib = 2048u64;
    let need = P::minimum(src).unwrap();
    let size = |s: u64| s.div_ceil(mib) + 8;
    for (role, sectors) in [
        (PartRole::Esp, need[0]),
        (PartRole::SlotA, need[1]),
        (PartRole::SlotB, need[2]),
        (PartRole::Data, need[3]),
    ] {
        assert!(w.manual_create(size(sectors)), "{role:?}");
        let i = w
            .layout
            .manual
            .iter()
            .position(|p| p.existing.is_none() && p.role == PartRole::Keep)
            .unwrap();
        w.manual_assign(i, role);
    }
}

#[test]
fn manual_layout_creates_deletes_assigns_and_validates() {
    use ultar_installer_ui::engine::{PartRole, Plan as P};
    let dir = scratch("manual");
    let src = medium(&dir);
    let mib = 2048u64;
    let tgt = foreign_target(&dir, 700 * MIB, &[("Old data", 65 * mib, 165 * mib - 1)]);
    let before = range_sha(&tgt, mib, 65 * mib - 1);
    let mut w = wizard_at(&src, vec![disk_info(&tgt, "Disk")], Step::Disk);
    w.layout.mode = ultar_installer_ui::wizard::DiskMode::Manual;
    w.replan();
    assert_eq!(w.blocked(), Some("installer.manual.need_esp"));
    assert_eq!(w.layout.manual.len(), 2);
    w.manual_delete(1);
    assert_eq!(w.layout.manual[1].role, PartRole::Delete);
    assign_all(&mut w, &src);
    let _ = P::minimum(&src).unwrap();
    assert!(!w.manual_create(10_000), "no room for 10 GiB");
    assert!(w.blocked().is_none(), "{:?}", w.plan);
    let slot_a = w
        .layout
        .manual
        .iter()
        .position(|p| p.role == PartRole::SlotA)
        .unwrap();
    w.manual_assign(slot_a, PartRole::Data);
    assert_eq!(w.blocked(), Some("installer.manual.twice"));
    w.manual_assign(slot_a, PartRole::SlotA);
    w.step = Step::Summary;
    w.secrets.password = [
        ultar_installer_ui::secret::Secret::new("km-secret-1"),
        ultar_installer_ui::secret::Secret::new("km-secret-1"),
    ];
    w.erase_confirmed = true;
    let (confirm, plan) = w.confirm_install().expect("confirm");
    assert_eq!(plan.removed.len(), 1);
    write::execute(&plan, &confirm, &mut |_, _| {}).expect("install");
    assert_eq!(
        range_sha(&tgt, mib, 65 * mib - 1),
        before,
        "Windows untouched"
    );
    let table = disk_info(&tgt, "Disk").table.unwrap();
    assert!(table.iter().any(|e| e.name == "Windows"));
    assert!(!table.iter().any(|e| e.name == "Old data"), "deleted");
    assert!(table.iter().any(|e| e.name == "ultar-system-a"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_changed_table_is_refused_before_writing() {
    let dir = scratch("changed");
    let src = medium(&dir);
    let tgt = foreign_target(&dir, 600 * MIB, &[]);
    let mut w = wizard_at(&src, vec![disk_info(&tgt, "Disk")], Step::Disk);
    w.layout.mode = ultar_installer_ui::wizard::DiskMode::Beside;
    w.replan();
    w.step = Step::Summary;
    w.secrets.password = [
        ultar_installer_ui::secret::Secret::new("km-secret-1"),
        ultar_installer_ui::secret::Secret::new("km-secret-1"),
    ];
    w.erase_confirmed = true;
    let (confirm, plan) = w.confirm_install().expect("confirm");
    // someone else adds a partition after the summary
    gpt_image(
        &tgt,
        600 * MIB,
        &[
            ("Windows", 2048, 65 * 2048 - 1),
            ("New", 65 * 2048, 66 * 2048),
        ],
    );
    let before = sha(&tgt);
    assert_eq!(
        write::execute(&plan, &confirm, &mut |_, _| {}),
        Err(EngineError::TargetChanged)
    );
    assert_eq!(sha(&tgt), before, "nothing written");
    std::fs::remove_dir_all(dir).unwrap();
}
