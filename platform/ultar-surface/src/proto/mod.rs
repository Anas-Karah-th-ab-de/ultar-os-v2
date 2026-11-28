//! The client ↔ compositor protocol: typed requests and events.
//!
//! One Unix stream connection per client. The first request is
//! [`Request::Hello`], the first event [`Event::Welcome`]. After that both
//! sides send frames in any order. Descriptors (`SCM_RIGHTS`) are accepted
//! only together with [`Request::CreateShmBuffer`].

mod event;
mod event_codec;
mod ime;
mod input_config;
mod key_helpers;
mod outputs;
mod request;
mod request_decode;
mod shell;
#[cfg(test)]
mod tests;
mod types;
mod visual;
mod windows;

pub use event::{Configure, ErrorCode, Event, OutputInfo};
pub use ime::ImeKey;
pub use input_config::{
    PointerSize, SwitchShortcut, DEFAULT_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS,
    MAX_KEY_REPEATS_PER_SECOND, MAX_KEY_REPEAT_DELAY_MS, MAX_POINTER_SPEED, MIN_DOUBLE_CLICK_MS,
    MIN_POINTER_SPEED,
};
pub use key_helpers::ComposeKey;
pub use outputs::{
    Arrangement, OutputDetails, OutputIdentity, OutputMode, OutputSetup, Setup, SetupOutcome,
    SetupRefusal, Transform, MAX_CONNECTOR, MAX_IDENTIFY_MS, MAX_OUTPUT_MODES, MAX_PRODUCT_NAME,
    MAX_PROVISIONAL_MS, MAX_SETUP_OUTPUTS,
};
pub use request::Request;
pub use shell::{
    DisplayModeRequest, ShellWindow, SwipeEdge, WindowAction, ALL_WORKSPACES, MAX_LAYOUTS,
    MAX_LAYOUT_NAME, MAX_SHELL_WINDOWS, MAX_THUMBNAILS_PER_SECOND, MAX_THUMBNAIL_SIDE,
    MAX_WORKSPACES,
};
pub use types::*;
pub use visual::{
    ColourFilter, LookLang, DEFAULT_ZOOM_PERCENT, MAX_FLASH_MS, MAX_ZOOM_PERCENT, MIN_ZOOM_PERCENT,
};
pub use windows::{WindowEntry, MAX_LISTED_WINDOWS};
