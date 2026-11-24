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
