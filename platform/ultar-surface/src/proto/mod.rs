//! The client ↔ compositor protocol: typed requests and events.
//!
//! One Unix stream connection per client. The first request is
//! [`Request::Hello`], the first event [`Event::Welcome`]. After that both
//! sides send frames in any order. Descriptors (`SCM_RIGHTS`) are accepted
//! only together with [`Request::CreateShmBuffer`].

mod event;
mod event_codec;
mod input_config;
mod outputs;
mod request;
mod request_decode;
mod shell;
#[cfg(test)]
mod tests;
mod types;
mod windows;

pub use event::{Configure, ErrorCode, Event, OutputInfo};
pub use input_config::{
    PointerSize, SwitchShortcut, DEFAULT_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS,
    MAX_KEY_REPEATS_PER_SECOND, MAX_KEY_REPEAT_DELAY_MS, MAX_POINTER_SPEED, MIN_DOUBLE_CLICK_MS,
    MIN_POINTER_SPEED,
};
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
pub use windows::{WindowEntry, MAX_LISTED_WINDOWS};
