//! `ultar-ui` – the one UI toolkit every Ultar OS surface is built from (plan §3.0, area 5
//! "platform"; design spec §3 and §4).
//!
//! Layers, bottom to top:
//! - [`tokens`]: design values generated from `tokens.toml`; no other module holds a colour or size.
//! - [`canvas`]: software renderer into an RGBA/BGRX buffer with clipping and damage tracking.
//! - [`text`]: Arabic-first text engine (shaping, BiDi, line breaking, glyph cache).
//! - [`icon`]: stroke icons from an SVG path subset.
//! - [`layout`]: logical (start/end) flex layout; direction resolves at paint time.
//! - [`widgets`], [`ui`], [`event`]: widget tree, state machine, keyboard/pointer/touch input,
//!   accessibility nodes, emitted [`event::Action`]s.
//! - [`snapshot`]: render a tree to PNG and compare with reference images.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod canvas;
pub mod color;
pub mod event;
pub mod geom;
pub mod icon;
pub mod layout;
pub mod paint;
#[cfg(feature = "snapshot")]
pub mod snapshot;
pub mod text;
pub mod tokens;
pub mod ui;
pub mod widgets;

pub use color::Color;
pub use geom::{PixelRect, Point, Rect, Size};
pub use ultar_i18n::{Direction, Lang};
