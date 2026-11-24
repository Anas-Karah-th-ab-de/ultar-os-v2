//! # ultar-hal-gfx – graphics backends of the compositor
//!
//! **Area:** system HAL (area 3, plan §3.0 / §3.1). The compositor
//! (`platform/ultar-compositor`) and its client library
//! (`platform/ultar-surface`) never see a device path or an `unsafe` block;
//! everything that touches the kernel directly lives here:
//!
//! | Module | What | Kernel interface |
//! |---|---|---|
//! | [`shm`] | pixel buffers shared between an app and the compositor | `memfd_create`, `F_ADD_SEALS`, `mmap(MAP_SHARED)` |
//! | [`fdpass`] | sending a buffer descriptor over a Unix socket | `sendmsg`/`recvmsg` with `SCM_RIGHTS` |
//! | [`kms`] | Linux DRM/KMS outputs with dumb buffers (development on Linux) | DRM ioctls through the pure-Rust `drm` crate |
//! | [`fbdev`] | the Ultar kernel's framebuffer `fb0` (and Linux fbdev) | `FBIOGET_*SCREENINFO`, `pwrite` |
//! | [`edid`] | what a display says about itself (maker, product, serial, name) | none (bytes from [`kms`]) |
//! | [`evdev`] | keyboards, mice, touch screens | `input_event` records, `EVIOCGBIT`/`EVIOCGABS` |
//!
//! [`kms::Kms`] and [`fbdev::FbDev`] implement [`ultar_hal::Display`];
//! [`evdev::Evdev`] implements [`ultar_hal::Input`]. All system calls go
//! through `rustix` (its `linux_raw` backend, no C library). The only
//! `unsafe` blocks are the memory mappings and the three device `ioctl`s,
//! each with a `SAFETY:` comment.
//!
//! **Ultar kernel:** `memfd_create` with seals, shared mappings and
//! `SCM_RIGHTS` exist since the compositor gap round K1–K13; on a kernel
//! without them [`shm::probe`] reports false and the compositor falls back to
//! pixels sent inline over the socket (see `docs/COMPOSITOR.md`, "Kernel gaps").

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod edid;
mod error;
pub mod evdev;
pub mod fbdev;
pub mod fdpass;
pub mod kms;
pub mod shm;
pub mod uevent;

pub use error::GfxError;
