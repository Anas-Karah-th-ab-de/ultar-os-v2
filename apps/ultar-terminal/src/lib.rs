//! # ultar-terminal – the terminal of Ultar OS
//!
//! A shell with commands in Arabic and English ([`shell`]) and the window around it
//! ([`app`]): the output as a picture drawn by [`ultar_textview::TextView`] (Arabic lines
//! from the right), a command line with history, Ctrl+C to stop a running program. Programs
//! of the system run with their output piped into the window ([`run`]); there is no
//! pseudo-terminal, so full-screen programs are not supported.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod i18n;
pub mod run;
pub mod shell;
