//! # ultar-editor – the text editor of Ultar OS
//!
//! Opens a text file of the user (`files://<area>/<path>`, handed over by Files), shows it in
//! a [`ultar_textview::TextView`] with line numbers and edits it with the buffer of the office
//! library (caret, selection, undo, encodings incl. Windows-1256). Arabic lines run right to
//! left, code left to right, in the same file.
//!
//! | Module | |
//! |---|---|
//! | [`doc`] | the document: buffer, line index, load and save |
//! | [`place`] | where a URI or a new document lives in the cell's areas |
//! | [`keys`] | what a key does to the document |
//! | [`app`] | the window: tool bar, text, status line, dialogs |

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod doc;
pub mod i18n;
pub mod keys;
pub mod place;
