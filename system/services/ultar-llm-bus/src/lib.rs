//! `ultar-llm-bus` – the language model on the system bus (area 4, system services).
//!
//! The model service (`ultar-llm`) speaks its own protocol on its own socket and decides
//! itself how it computes – processor, graphics chip, neural chip. This service changes
//! nothing about that: it only offers the contract `llm.state` / `llm.complete` on the bus
//! and passes each call on, so the helper and the mesh can use a model without knowing the
//! model service.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod backend;
pub mod service;

pub use service::LlmBus;
