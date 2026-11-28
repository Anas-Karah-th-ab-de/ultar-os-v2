//! Growth against e2fsprogs: images from mke2fs, grown, then `e2fsck -fn` must find nothing,
//! the files must still be there and `dumpe2fs` must report the new size. Skipped without mke2fs.

use std::path::{Path, PathBuf};
use std::process::Command;
use ultar_ext4grow::{grow, Outcome};

fn have(tool: &str) -> bool {
    Command::new(tool).arg("-V").output().is_ok()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ext4grow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("tree/sub")).unwrap();
    std::fs::write(d.join("tree/probe.txt"), b"ultar data probe\n").unwrap();
    std::fs::write(d.join("tree/sub/big.bin"), vec![7u8; 3 << 20]).unwrap();
    d
}

fn image(dir: &Path, mib: u64, extra: &[&str]) -> PathBuf {
    let img = dir.join("fs.img");
    let ok = Command::new("mke2fs")
        .args(["-q", "-F", "-t", "ext4"])
        .args(extra)
        .arg("-d")
        .arg(dir.join("tree"))
        .arg(&img)
        .arg(format!("{mib}M"))
        .status()
        .unwrap()
        .success();
    assert!(ok, "mke2fs");
    img
}

fn grow_to(img: &Path, bytes: u64) -> Outcome {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(img)
        .unwrap();
    f.set_len(bytes).unwrap();
    grow(&mut f, bytes).unwrap()
}

fn fsck_clean(img: &Path) {
    let out = Command::new("e2fsck")
        .args(["-fn"])
        .arg(img)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "e2fsck:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn blocks(img: &Path) -> u64 {
    let out = Command::new("dumpe2fs")
        .arg("-h")
        .arg(img)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text
        .lines()
        .find(|l| l.starts_with("Block count:"))
        .unwrap();
    line.split_whitespace().last().unwrap().parse().unwrap()
}

fn probe(img: &Path) -> String {
    let out = Command::new("debugfs")
        .args(["-R", "cat /probe.txt"])
        .arg(img)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn case(name: &str, mib: u64, extra: &[&str], to: u64) {
    if !have("mke2fs") || !have("e2fsck") {
        eprintln!("note: e2fsprogs missing, {name} skipped");
        return;
    }
    let dir = scratch(name);
    let img = image(&dir, mib, extra);
    let before = blocks(&img);
    let r = grow_to(&img, to);
    assert!(matches!(r, Outcome::Grown { .. }), "{name}: {r:?}");
    fsck_clean(&img);
    assert!(blocks(&img) > before, "{name}: grown");
    assert!(
        probe(&img).contains("ultar data probe"),
        "{name}: files kept"
    );
    // libext2fs allocates in the new groups (a file larger than the old file system)
    let big = dir.join("big.bin");
    std::fs::write(&big, vec![0x5Au8; ((mib + 40) << 20) as usize]).unwrap();
    let cmd = format!("write {} /big", big.display());
    let ok = Command::new("debugfs")
        .args(["-w", "-R", &cmd])
        .arg(&img)
        .output()
        .unwrap();
    assert!(ok.status.success(), "{name}: debugfs write");
    fsck_clean(&img);
    let out = Command::new("debugfs")
        .args(["-R", "stat /big"])
        .arg(&img)
        .output()
        .unwrap();
    let want = format!("Size: {}", (mib + 40) << 20);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&want),
        "{name}: the large file is complete"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn grows_the_default_layout_many_groups() {
    case("default", 64, &[], 3 << 30);
}

#[test]
fn grows_into_new_backup_groups_and_reserved_descriptor_blocks() {
    // 4 KiB blocks, 1 descriptor block: 3 GiB → 24 groups (backups in 3, 5, 7, 9);
    // 40 GiB → 320 groups → 5 descriptor blocks taken from the reserved ones
    case("many", 32, &[], 40 << 30);
}

#[test]
fn grows_a_partial_last_group() {
    case("partial", 200, &[], 1 << 30);
}

#[test]
fn grows_1k_blocks() {
    case("1k", 16, &["-b", "1024"], 512 << 20);
}

#[test]
fn grows_32bit_descriptors() {
    case("32bit", 64, &["-O", "^64bit"], 2 << 30);
}

#[test]
fn grows_without_flex_bg() {
    case("noflex", 64, &["-O", "^flex_bg"], 2 << 30);
}

#[test]
fn leaves_unknown_layouts_alone() {
    if !have("mke2fs") {
        return;
    }
    let dir = scratch("meta");
    let img = image(&dir, 64, &["-O", "meta_bg,^resize_inode"]);
    assert!(matches!(grow_to(&img, 1 << 30), Outcome::Unchanged(_)));
    fsck_clean(&img);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn growing_twice_and_to_the_same_size() {
    if !have("mke2fs") {
        return;
    }
    let dir = scratch("twice");
    let img = image(&dir, 64, &[]);
    assert!(matches!(grow_to(&img, 1 << 30), Outcome::Grown { .. }));
    assert!(matches!(grow_to(&img, 1 << 30), Outcome::Unchanged(_)));
    assert!(matches!(grow_to(&img, 5 << 30), Outcome::Grown { .. }));
    fsck_clean(&img);
    std::fs::remove_dir_all(dir).unwrap();
}
