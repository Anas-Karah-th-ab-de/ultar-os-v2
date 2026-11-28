//! # ultar-pdf – the PDF reader of Ultar OS
//!
//! Opens a PDF of the user (`files://<area>/<path>[#page=N]`, handed over by Files), shows
//! the pages continuously with zoom, fit and rotate, searches with every hit highlighted and
//! lets the user select and copy text. Rendering runs in a worker thread with a memory-limited
//! cache, so a long document stays smooth. Arabic first: the interface mirrors in RTL, the page
//! itself stays as the document lays it out.
//!
//! | Module | |
//! |---|---|
//! | [`state`] | the pure logic: zoom, page geometry, rotation, search, selection |
//! | [`doc`] | the document: loading, the render worker and the picture cache |
//! | [`keys`] | what a key does (shortcuts) |
//! | [`place`] | where a URI lives, its page, and the recent files |
//! | [`app`] | the window: tool bar, page view, side panel, search, dialogs |

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod doc;
pub mod i18n;
pub mod keys;
pub mod place;
pub mod service;
pub mod state;