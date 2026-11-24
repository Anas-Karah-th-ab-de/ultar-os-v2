//! The client ↔ compositor protocol: typed requests and events.
//!
//! One Unix stream connection per client. The first request is
//! [`Request::Hello`], the first event [`Event::Welcome`]. After that both
//! sides send frames in any order. Descriptors (`SCM_RIGHTS`) are accepted
//! only together with [`Request::CreateShmBuffer`].

mod event;
mod event_codec;
mod request;
mod request_decode;
mod shell;
#[cfg(test)]
mod tests;
mod types;
mod windows;

pub use event::{Configure, ErrorCode, Event, OutputInfo};
pub use request::Request;
pub use shell::{
    DisplayModeRequest, ShellWindow, WindowAction, MAX_LAYOUTS, MAX_LAYOUT_NAME, MAX_SHELL_WINDOWS,
};
pub use types::*;
pub use windows::{WindowEntry, MAX_LISTED_WINDOWS};
