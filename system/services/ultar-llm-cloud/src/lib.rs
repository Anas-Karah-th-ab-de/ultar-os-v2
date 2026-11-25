//! `ultar-llm-cloud` – a provider's language model on the system bus (area 4, system
//! services): the helper's **last** way to think.
//!
//! The helper prefers this device's own model, then a model a paired device lends; only
//! when neither exists does it ask here – and here answers only after the person entered
//! a key of their own (Settings › Assistant). The key lies in this service's own folder,
//! readable for this service only, and no call gives it out.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod key;
pub mod provider;
pub mod service;

pub use service::LlmCloud;
