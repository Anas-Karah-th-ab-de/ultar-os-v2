//! `ultar-osk` – the on-screen keyboard of Ultar OS.
//!
//! ```text
//! ultar-osk [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]
//! ```
//! Runs as its own user with the compositor right `ui.input-method`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_desktop::app::{osk, parse_options};
use ultar_service::{exit, Runtime, ServiceSpec};

fn main() -> ExitCode {
    let Some((options, rest)) = parse_options(std::env::args().skip(1)) else {
        return ExitCode::from(exit::USAGE as u8);
    };
    if !rest.is_empty() {
        return ExitCode::from(exit::USAGE as u8);
    }
    let Ok(runtime) = Runtime::start(ServiceSpec::new("ultar-osk")) else {
        return ExitCode::from(exit::HARDENING as u8);
    };
    let code = runtime.run(|rt| {
        rt.ready();
        let stop = || rt.lifecycle().is_stopping();
        match osk::run(&options, &stop) {
            Ok(()) => exit::OK,
            Err(_) => exit::FAILURE,
        }
    });
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
