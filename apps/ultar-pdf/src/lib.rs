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
//! | [`state`], [`layout`] | the pure logic: zoom, fit, rotation, page geometry |
//! | [`text`], [`search`] | the text layer of a page, search with Arabic normalisation, selection |
//! | [`doc`], [`worker`], [`cache`] | the document, its render worker and the picture cache |
//! | [`app`], [`build`], [`paint`], [`input`] | the window: state, tree, pictures, keys/pointer/touch |
//! | [`keys`], [`touch`] | the shortcut map and the finger gestures |
//! | [`action`] | what the helper may do (APP-RULES §9) |
//! | [`place`], [`service`] | where a URI lives and the recent files; the mesh report |

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod action;
pub mod app;
pub mod build;
pub mod cache;
pub mod doc;
pub mod i18n;
pub mod input;
pub mod keys;
pub mod layout;
pub mod paint;
pub mod place;
pub mod raster;
pub mod search;
pub mod service;
pub mod state;
pub mod text;
pub mod touch;
pub mod worker;
