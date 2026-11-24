//! `ultar-audiod` – the audio service of Ultar OS (area 4, system services).
//!
//! Drives the playback controls of the sound cards through the HAL
//! ([`ultar_hal_linux::CardControls`]): volume and mute of the output in use, the
//! choice of output, `audio.state` (retained). The chosen output and each card's volume
//! and mute are kept in a checked state file and restored at start (the kernel starts
//! every card at its driver default). Cards that come and go (USB) are noticed within
//! [`service::TICK`]. Without a sound card the state says `present: false` and every
//! change answers `NoDevice`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod mixer;
pub mod service;

pub use mixer::Mixer;
pub use service::AudioService;
