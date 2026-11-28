//! # ultar-sheet – the spreadsheet of Ultar OS (Excel)
//!
//! Opens and saves `.xlsx` and `.csv` (UTF-8 and Windows-1256), shows the sheets as a grid that
//! only draws what is on screen, edits in the cell and in a formula bar, calculates by
//! dependency ([`ultar_office::calc`]), formats cells, and turns a sheet around for Arabic
//! (column A at the right). The parts:
//!
//! | Module | Job |
//! |---|---|
//! | [`file`] | open / save, atomic, encodings, reasons for failure |
//! | [`recover`] | the copy for crash recovery |
//! | [`book`] | workbook, undo, recalculation |
//! | [`sel`], [`core`], [`ops`] | selection, keys, the commands of the user |
//! | [`frame`], [`draw`] | the visible geometry and the picture of the grid |
//! | [`agent`] | what the assistant may do |
//! | [`app`] | the window: tool bar, formula bar, grid, tabs, dialogs |
//! | [`place`], [`i18n`] | `files://` URIs and the app's texts |

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod agent;
pub mod app;
pub mod ask;
pub mod book;
pub mod cmd;
pub mod core;
pub mod input;
pub mod draw;
pub mod file;
pub mod frame;
pub mod i18n;
pub mod ops;
pub mod place;
pub mod recover;
pub mod sel;
