//! `ultar-i18n` – the one translation and locale system of Ultar OS v2.
//!
//! **Area:** system core (area 3 of the plan, `system/kern/`). Every other area – boot chain, kernel
//! emergency console, installer, services, platform, apps and CLI tools – takes its user-visible text
//! from here instead of carrying its own tables.
//!
//! Arabic is the reference language; English and German follow. The crate offers:
//!
//! * message catalogs in a small Fluent-like text format ([`Catalog`], [`Message`]),
//! * a [`Translator`] with the fallback chain *requested language → Arabic → key itself*,
//! * plural rules ([`plural`]) including the six Arabic forms,
//! * text direction and logical layout sides ([`Direction`], [`Side`]),
//! * numbers that always use Western digits 0–9 ([`numbers`]),
//! * Gregorian and Hijri calendar helpers ([`calendar`], [`datefmt`]),
//! * Arabic text utilities for search and sorting ([`arabic`], [`collation`]),
//! * catalog completeness checks ([`check`], and [`check_dir`] with the `std` feature).
//!
//! The core is `no_std` + `alloc`; the default feature `std` only adds environment detection and the
//! directory checker used by the `ultar-i18n-check` binary.
//!
//! ```
//! use ultar_i18n::{Arg, Lang, Translator};
//!
//! let tr = Translator::new(Lang::Ar);
//! assert_eq!(tr.t("action.cancel"), "إلغاء");
//! assert_eq!(tr.t_args("count.files", &[("count", Arg::Int(2))]), "ملفان");
//! ```
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

extern crate alloc;

pub mod arabic;
pub mod bidi;
pub mod calendar;
pub mod catalog;
pub mod check;
#[cfg(feature = "std")]
pub mod check_dir;
pub mod collation;
pub mod datefmt;
pub mod embedded;
pub mod error;
pub mod lang;
pub mod numbers;
mod parser;
pub mod pattern;
pub mod plural;
pub mod translator;
pub mod units;

pub use arabic::normalize_for_search;
pub use bidi::{contains_rtl, first_strong_direction};
pub use calendar::{Date, HijriDate, WeekStart, Weekday};
pub use catalog::Catalog;
pub use check::{missing_keys, placeholder_mismatches, PlaceholderMismatch};
pub use collation::{collation_key, compare, CollationKey};
pub use error::{ParseError, ParseErrorKind};
pub use lang::{direction, Direction, Lang, PhysicalSide, Side};
pub use pattern::{Arg, Message};
pub use plural::PluralCategory;
pub use translator::Translator;
