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
#[cfg(test)]
mod tests;
mod types;

pub use event::{Configure, ErrorCode, Event, OutputInfo};
pub use request::Request;
pub use types::*;
