//! `ultar-mobile` – the phone shell of Ultar OS: status bar, navigation bar, home screen,
//! quick settings, recent apps and search.
//!
//! ```text
//! ultar-mobile [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]
//! ```
//! Runs as its own user with the compositor right `ui.shell`; `SIGTERM` ends it.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_mobile::program::{parse_options, shell};
use ultar_service::{exit, Runtime, ServiceSpec};

fn main() -> ExitCode {
    let Some(options) = parse_options(std::env::args().skip(1)) else {
        return ExitCode::from(exit::USAGE as u8);
    };
    let Ok(runtime) = Runtime::start(ServiceSpec::new("ultar-mobile")) else {
        return ExitCode::from(exit::HARDENING as u8);
    };
    let code = runtime.run(|rt| {
        rt.ready();
        let stop = || rt.lifecycle().is_stopping();
        match shell::run(&options, &stop) {
            Ok(()) => exit::OK,
            Err(_) => exit::FAILURE,
        }
    });
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
