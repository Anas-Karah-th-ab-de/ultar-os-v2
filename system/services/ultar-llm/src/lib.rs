//! ultar-llm – lokales Sprachmodell als Dienst im Rust-Kern von Ultar OS (Handy und Desktop).
//! Doku: docs/LLM-DIENST.md

pub mod client;
pub mod config;
pub mod engine;
pub mod http;
#[cfg(feature = "llama")]
pub mod llama;
pub mod models;
pub mod platform;
pub mod service;
