//! `ultar-calendard` – the calendar service (see the library documentation).
//!
//! ```text
//! ultar-calendard --bus PATH --dir DIR [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_calendard::CalendarService;
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    ultar_svckit::main("ultar-calendard", &["dir"], None, |args, _| {
        let dir = args.required_path("dir").map_err(|_| Msg::BadArguments)?;
        Ok(CalendarService::new(dir))
    })
}
