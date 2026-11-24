//! `ultar-captured` – the screenshot service of Ultar OS (area 5, platform).
//!
//! `capture.screenshot` (the shell's button): reads the pixels of one output from the
//! compositor (right `ui.capture`, granted to this service's user only), encodes a PNG
//! and saves it as `Pictures/Screenshot-<n>.png` in the home of the person in front
//! (`session.state` + `account.users` for the id), owned by that person, mode 0600.
//! Links are never followed; an existing name is never overwritten. One screenshot at a
//! time; the compositor sends nothing while the session is locked (answer `Failed`).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod grab;
pub mod save;
pub mod service;

pub use service::CaptureService;
