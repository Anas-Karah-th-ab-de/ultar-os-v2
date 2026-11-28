//! # ultar-doc - the word processor of Ultar OS
//!
//! Opens, edits and saves `.docx` documents (and plain `.txt`), with paragraphs mixing Arabic
//! and Latin (right-to-left per paragraph, a direction button), formatting
//! (bold/italic/underline/strikethrough, size, colour, highlight, headings, alignment, line
//! spacing, lists, indent), tables and pictures, undo/redo, find/replace, word count, zoom and
//! export to PDF. The model and the `.docx` reading/writing live in `ultar-office`; the page
//! layout, the drawing and the editing live here.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod action;
pub mod app;
pub mod commands;
pub mod dialogs;
pub mod edit;
pub mod export;
pub mod files;
pub mod format;
pub mod history;
pub mod i18n;
pub mod keys;
pub mod layout;
pub mod page;
pub mod paint;
pub mod picture;
pub mod place;
pub mod pointer;
pub mod recovery;
pub mod search;
pub mod service;
pub mod state;
pub mod tables;

pub use action::Out;
pub use app::App;
