//! `ultar-volumed` – the removable-drive service of Ultar OS (area 4, system services).
//!
//! Looks for partitions of removable disks with FAT, exFAT or ext4 every 2 s (HAL
//! [`ultar_hal_linux::block`]) and mounts them for the person in front (`session.state`,
//! their id from `account.users`) below `<media>/<name>/<label>` with
//! `nosuid,nodev,noexec`; FAT/exFAT files belong to that person only (`0077` masks).
//! `volumes.state` (retained) lists the drives of the person in front;
//! `volumes.eject` (the shell's drive tile) unmounts – refused while files are open – so
//! the drive can be pulled out. A pulled drive is detached at once. Needs the kernel
//! right `mount`; without it nothing is mounted and nothing is shown.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod service;

pub use service::VolumeService;
