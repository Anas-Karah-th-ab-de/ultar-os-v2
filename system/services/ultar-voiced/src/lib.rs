//! `ultar-voiced` – voice control of the helper (area 4, system services).
//!
//! The person switches it on in Settings › Assistant › Sprache. While it is on the service
//! listens for the wake word (standard "Hey KM", or the person's own word), records the
//! request until silence (at most 15 s), turns it into text, sends it to the helper as
//! `agent.ask_voice`, and speaks the answer aloud. Everything runs on the device; nothing
//! is stored except the person's enrolment data (the wake word template, the secret word
//! template and the voice print), sealed in the service's state directory.
//!
//! It is **off by default**, records only after the wake word, never stores a request, and
//! publishes `voice.state` so the shell can show the on-screen mark while it listens. While
//! the screen is locked only the secret word wakes it (never the wake word), and the helper
//! still answers only harmless things without an unlock (its own confirmation rule).
//!
//! The signal processing is pure Rust ([`mfcc`], [`detect`]): a mel-frequency template of
//! the wake and secret words matched by cosine distance, and a voice print (the same
//! embedding over the enrolment sentence) used to refuse other speakers. Speech-to-text and
//! text-to-speech are the `whisper` and `espeak` packages built by `ultar-paketbau`; their
//! processes are started only when needed ([`engine`]). Without those packages the service
//! answers every such attempt with a clear "missing" outcome and keeps working otherwise.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod audio;
pub mod detect;
pub mod engine;
pub mod mfcc;
pub mod pipeline;
pub mod service;
pub mod state;

pub use service::VoiceService;