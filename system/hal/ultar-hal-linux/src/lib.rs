//! `ultar-hal-linux` – generic kernel backends of the HAL for the system services (area 3).
//!
//! The Ultar kernel and Linux name these interfaces the same way, so one backend serves
//! both; whatever a kernel does not offer is reported as absent ([`HalError::NotSupported`]
//! or an empty list), never imitated. This crate is the only place of the services where
//! `/sys`, `/proc` and `/dev` paths appear; every backend takes a root directory so tests
//! run against a prepared tree.
//!
//! | Backend | Kernel interface | Module |
//! |---|---|---|
//! | Battery, chargers, temperatures, sleep | `sys/class/power_supply`, `sys/class/thermal`, `sys/power/state` | [`power`] |
//! | Sound cards: volume and mute of the playback controls | `dev/snd/controlC*` (ALSA control ioctls) | [`sound`] |
//! | Network interfaces: wired link, speed, Wi-Fi interfaces | `sys/class/net` | [`net`] |
//! | Removable drives: partitions, file system, label, mounting | `sys/block`, `dev/<name>` (superblocks), `mount(2)` | [`block`] |
//! | Radio switches: Wi-Fi, Bluetooth, mobile, airplane mode | `sys/class/rfkill` (state), `dev/rfkill` (changes) | [`rfkill`] |
//! | Bluetooth controllers: list, raw HCI user channel (Linux only) | `sys/class/bluetooth`, `AF_BLUETOOTH` socket | [`bluetooth`] |
//! | Virtual keyboards and mice (Linux only) | `dev/uinput` | [`uinput`] |
//!
//! `unsafe` is denied everywhere but in `sound/ioctl.rs` (four ALSA control ioctls on
//! fixed-size buffers that hold no pointers), `bluetooth.rs` (the `sockaddr_hci` of the
//! user channel) and `uinput.rs` (five ioctls with integer or no arguments), each block
//! with a `SAFETY` comment.
//!
//! [`HalError::NotSupported`]: ultar_hal::HalError::NotSupported
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(unsafe_code)]

pub mod block;
pub mod bluetooth;
pub mod net;
pub mod power;
pub mod rfkill;
pub mod sound;
mod sysfs;
pub mod uinput;

pub use power::LinuxPower;
pub use sound::{Card, CardControls, SoundCards};
