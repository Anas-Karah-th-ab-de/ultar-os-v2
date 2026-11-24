//! # ultar-web – the Ultar Browser
//!
//! **Area:** apps (area 7, plan §3.0). The browser process: tabs, address field with search,
//! history, downloads, site permissions, tracking protection and the browser window, drawn
//! with `ultar-ui` and shown through the compositor (`ultar-surface`). Web pages are rendered by
//! an engine behind [`engine::WebEngine`]:
//!
//! * [`engine::remote::RemoteEngine`] – the WPE WebKit engine host (`external/wpe-webkit`),
//!   a separate process that runs in its own cell ([`cell`]) with network, without the
//!   user's files except the downloads folder; frames come through shared memory.
//! * [`engine::test_engine::TestEngine`] – a deterministic in-process engine for headless
//!   tests (pages are solid colours, history, downloads and permission requests are scripted).
//!
//! The browser process itself holds no engine code and no C library: it is a static Rust
//! program. Design: `docs/integration/ultar-web.md`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod app;
pub mod catalog;
pub mod cell;
pub mod chrome;
pub mod engine;
pub mod filter;
pub mod ipc;
pub mod model;
pub mod url;
pub mod window;
