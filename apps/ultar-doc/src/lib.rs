//! # ultar-doc – the word processor of Ultar OS
//!
//! Opens, edits and saves `.docx` documents (and plain `.txt`), with paragraphs mixing Arabic
//! and Latin (right-to-left per paragraph, a direction button), formatting
//! (bold/italic/underline/strikethrough, size, colour, highlight, headings, alignment, line
//! spacing, lists, indent), tables and pictures, undo/redo, find/replace, word count and zoom.
//! The model and the `.docx` reading/writing live in `ultar-office`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod action;
pub mod app;
pub mod i18n;
pub mod keys;
pub mod place;
pub mod service;
pub mod state;