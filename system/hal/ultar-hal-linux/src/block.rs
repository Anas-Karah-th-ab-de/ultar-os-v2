//! Removable drives: which partitions of removable disks exist, their file system and
//! label (read from the superblock), and mounting them without set-id programs,
//! device files or execution (`nosuid,nodev,noexec`).

use crate::sysfs::{entries, number};
use std::io::Read;
use std::path::{Path, PathBuf};
use ultar_hal::{HalError, HalResult};

/// File systems a removable drive is mounted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsKind {
    /// FAT12/16/32.
    Vfat,
    /// exFAT.
    Exfat,
    /// ext2/3/4.
    Ext4,
}

impl FsKind {
    /// Name for `mount(2)`.
    pub fn name(self) -> &'static str {
        match self {
            FsKind::Vfat => "vfat",
            FsKind::Exfat => "exfat",
            FsKind::Ext4 => "ext4",
        }
    }
}

/// One partition of a removable disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// Kernel name (`sdb1`).
    pub device: String,
    /// File system.
    pub fs: FsKind,
    /// Label (data; empty when none).
    pub label: String,
}

/// Reads what the first 2 KiB of a partition say: file system and label.
pub fn identify(head: &[u8]) -> Option<(FsKind, String)> {
    let text = |range: std::ops::Range<usize>| -> String {
        head.get(range)
            .map(|b| {
                String::from_utf8_lossy(b)
                    .trim_end_matches([' ', '\0'])
                    .to_owned()
            })
            .unwrap_or_default()
    };
    if head.get(3..11) == Some(b"EXFAT   ") {
        return Some((FsKind::Exfat, String::new()));
    }
    if head.get(1080..1082) == Some(&[0x53, 0xEF]) {
        return Some((FsKind::Ext4, text(1144..1160)));
    }
    if head.get(510..512) == Some(&[0x55, 0xAA]) {
        if head.get(82..87) == Some(b"FAT32") {
            return Some((FsKind::Vfat, text(71..82)));
        }
        if matches!(head.get(54..59), Some(b"FAT12") | Some(b"FAT16")) {
            return Some((FsKind::Vfat, text(43..54)));
        }
    }
    None
}

/// The removable drives below a root directory.
#[derive(Clone, Debug)]
pub struct Drives {
    root: PathBuf,
}

/// Longest label shown.
const MAX_LABEL: usize = 64;

impl Drives {
    /// Drives below `root` (`/` on a device).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn head(&self, device: &str) -> Option<Vec<u8>> {
        let file = std::fs::File::open(self.root.join("dev").join(device)).ok()?;
        let mut head = Vec::new();
        file.take(2048).read_to_end(&mut head).ok()?;
        Some(head)
    }

    /// Partitions (or whole disks without partition table) of removable disks with a
    /// known file system.
    pub fn volumes(&self) -> Vec<Volume> {
        let mut out = Vec::new();
        for disk in entries(&self.root.join("sys/block"), 64) {
            if number(&disk.join("removable")) != Some(1) {
                continue;
            }
            let Some(name) = disk.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
                continue;
            };
            let mut parts: Vec<String> = entries(&disk, 256)
                .iter()
                .filter(|p| p.join("partition").exists())
                .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
                .collect();
            if parts.is_empty() {
                parts.push(name);
            }
            for device in parts {
                if let Some((fs, mut label)) = self.head(&device).and_then(|h| identify(&h)) {
                    label.truncate(MAX_LABEL);
                    out.push(Volume { device, fs, label });
                }
            }
        }
        out
    }

    /// Mounts `volume` at `target` for the person `uid` (FAT and exFAT files belong to
    /// them; ext4 keeps its owners).
    pub fn mount(&self, volume: &Volume, target: &Path, uid: u32) -> HalResult<()> {
        use rustix::mount::{mount, MountFlags};
        let flags = MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC;
        let data = match volume.fs {
            FsKind::Vfat | FsKind::Exfat => format!("uid={uid},gid={uid},fmask=0077,dmask=0077"),
            FsKind::Ext4 => String::new(),
        };
        let data = std::ffi::CString::new(data).map_err(|_| HalError::InvalidArgument)?;
        mount(
            self.root.join("dev").join(&volume.device),
            target,
            volume.fs.name(),
            flags,
            data.as_c_str(),
        )
        .map_err(|e| match e {
            rustix::io::Errno::PERM | rustix::io::Errno::ACCESS => HalError::PermissionDenied,
            rustix::io::Errno::NODEV | rustix::io::Errno::NOSYS => HalError::NotSupported,
            rustix::io::Errno::BUSY => HalError::Busy,
            other => HalError::Io(other.raw_os_error()),
        })
    }

    /// Unmounts `target` (fails with `Busy` while files are open on it).
    pub fn unmount(&self, target: &Path) -> HalResult<()> {
        rustix::mount::unmount(target, rustix::mount::UnmountFlags::empty()).map_err(|e| match e {
            rustix::io::Errno::BUSY => HalError::Busy,
            rustix::io::Errno::PERM => HalError::PermissionDenied,
            other => HalError::Io(other.raw_os_error()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fat32(label: &[u8; 11]) -> Vec<u8> {
        let mut h = vec![0u8; 2048];
        h[71..82].copy_from_slice(label);
        h[82..87].copy_from_slice(b"FAT32");
        h[510] = 0x55;
        h[511] = 0xAA;
        h
    }

    #[test]
    fn file_systems_and_labels() {
        assert_eq!(
            identify(&fat32(b"STICK      ")),
            Some((FsKind::Vfat, "STICK".into()))
        );
        let mut ext = vec![0u8; 2048];
        ext[1080] = 0x53;
        ext[1081] = 0xEF;
        ext[1144..1149].copy_from_slice(b"Daten");
        assert_eq!(identify(&ext), Some((FsKind::Ext4, "Daten".into())));
        let mut ex = vec![0u8; 512];
        ex[3..11].copy_from_slice(b"EXFAT   ");
        assert_eq!(identify(&ex).map(|v| v.0), Some(FsKind::Exfat));
        assert_eq!(identify(&[0u8; 2048]), None);
        assert_eq!(identify(&[]), None);
        assert_eq!(FsKind::Exfat.name(), "exfat");
    }

    #[test]
    fn removable_partitions_are_found() {
        let root = std::env::temp_dir().join(format!("hal-block-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let put = |p: &str, v: &[u8]| {
            let p = root.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, v).unwrap();
        };
        put("sys/block/sda/removable", b"0\n");
        put("sys/block/sda/sda1/partition", b"1\n");
        put("dev/sda1", &fat32(b"SYSTEM     "));
        put("sys/block/sdb/removable", b"1\n");
        put("sys/block/sdb/sdb1/partition", b"1\n");
        put("sys/block/sdb/sdb2/partition", b"2\n");
        put("dev/sdb1", &fat32(b"STICK      "));
        put("dev/sdb2", &[0u8; 100]);
        put("sys/block/sdc/removable", b"1\n");
        put("dev/sdc", &fat32(b"CARD       "));
        let v = Drives::new(&root).volumes();
        let names: Vec<_> = v
            .iter()
            .map(|v| (v.device.as_str(), v.label.as_str()))
            .collect();
        assert_eq!(names, [("sdb1", "STICK"), ("sdc", "CARD")]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fuzzed_superblocks_never_panic() {
        let mut seed = 0x51_7cc1_b727_220a_u64;
        for _ in 0..3000 {
            let len = (seed % 2100) as usize;
            let mut h = vec![0u8; len];
            for b in h.iter_mut() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                *b = seed as u8;
            }
            if len > 512 {
                h[510] = 0x55;
                h[511] = 0xAA;
            }
            let _ = identify(&h);
        }
    }
}
