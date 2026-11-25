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
//!
//! It follows the Storage page settings (`settings.get` at start, `settings.changed` at
//! runtime): `storage.low_space_percent` warns once when the data partition runs low,
//! `storage.automount` off offers a drive with a "Mount" button instead of mounting it,
//! and `storage.clean_downloads` (+ `_days`) cleans the person's downloads folder once a
//! day. `storage.trim` and `storage.clean_old_updates` are not followed (nothing here can
//! put them into effect; docs/integration/settings-wiring.md says why).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod clean;
mod follow;
pub mod jobs;
mod offer;
pub mod service;
pub mod settings;

pub use service::{space_of, VolumeService};
pub use settings::StorageSettings;
