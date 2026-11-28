//! `ultar-voiced` – voice control of the helper (area 4, system services).
//!
//! The person switches it on in Settings › Assistant › Sprache. While it is on the service
//! hears the microphone stream of `ultar-audiod` and looks for the wake word (the standard
//! "Hey KM", or the secret word of the person), records the request after it until silence (at
//! most 15 s), turns it into text, sends it to the helper as `agent.ask_voice`, and speaks the
//! answer aloud through `ultar-audiod`. Everything runs on the device; nothing is stored
//! except the person's enrolment data (the word templates and the voice print), in the
//! service's own state directory.
//!
//! It is **off by default**, never records a request before the wake word, never stores one,
//! and publishes `voice.state` so the shell can show the on-screen mark while it listens. While
//! the screen is locked only the secret word wakes it (never the standard word), and the helper
//! then only does harmless things (`agent.ask_voice` with `locked`).
//!
//! The signal processing is pure Rust: mel-frequency cepstral coefficients ([`mfcc`]), voice
//! activity ([`vad`]), word templates matched by dynamic time warping and a voice print
//! ([`features`], [`detect`]). Speech to text is `whisper.cpp` and speech output eSpeak NG, both
//! packages built by `ultar-paketbau`; their processes are started only when needed
//! ([`whisper`], [`espeak`]).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod audio;
pub mod detect;
pub mod engine;
pub mod espeak;
pub mod features;
pub mod http;
pub mod job;
pub mod mfcc;
pub mod models;
mod parts;
pub mod pipeline;
pub mod service;
pub mod speech;
pub mod state;
pub mod vad;
pub mod whisper;

pub use service::VoiceService;
