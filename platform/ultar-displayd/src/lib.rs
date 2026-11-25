//! # ultar-displayd – the display service of Ultar OS
//!
//! **Area:** platform (area 5, plan §3.0): it sits between the settings
//! service (`settingsd`, system core) and the compositor (platform) and
//! owns the contract `display.*` (`ultar_bus::messages::display_service`).
//!
//! * It learns every connected display from the compositor – with the
//!   identity from the display's own data (EDID) and every mode – and tells
//!   `settingsd`, which remembers the settings per display combination
//!   keyed by those identities (the office monitor keeps its settings on any
//!   port; the dock at home is another combination than the projector).
//! * It turns the display settings into one arrangement (extend, mirror,
//!   one display; main display; modes; positions) and sends it to the
//!   compositor with the narrow `ui.output-config` right.
//! * `display.apply` (Super+P, main display, dragging, mode) makes the
//!   changes provisional in `settingsd` ("Keep?", 15 s); `display.keep`
//!   and `display.revert` answer them. A change nobody confirms goes back –
//!   also when the asking app, this service or `settingsd` ends (see
//!   [`core`]).
//! * `display.identify` shows the numbers on the displays, `display.detect`
//!   looks for displays again, `display.outputs` (retained) lists them.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod changes;
pub mod core;
pub mod daemon;
pub mod ids;
pub mod input;
pub mod key_helpers;
pub mod link;
pub mod msg;
pub mod plan;
pub mod ports;
pub mod service;
pub mod values;
pub mod visual;
