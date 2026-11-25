//! `ultar-mobile-lock` – the phone's lock screen: cover and sign-in. Lifts the lock only
//! after the authentication service said yes (shared lock state `ultar-lock-core`).
//!
//! ```text
//! ultar-mobile-lock [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light] [--user NAME]
//! ```
//! Runs as its own user with the compositor right `ui.lock-screen`; `SIGTERM` ends it.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_mobile::program::{lock, parse_options};
use ultar_service::{exit, Runtime, ServiceSpec};

fn main() -> ExitCode {
    let Some(options) = parse_options(std::env::args().skip(1)) else {
        return ExitCode::from(exit::USAGE as u8);
    };
    let Ok(runtime) = Runtime::start(ServiceSpec::new("ultar-mobile-lock")) else {
        return ExitCode::from(exit::HARDENING as u8);
    };
    let code = runtime.run(|rt| {
        rt.ready();
        let stop = || rt.lifecycle().is_stopping();
        match lock::run(&options, &stop) {
            Ok(()) => exit::OK,
            Err(_) => exit::FAILURE,
        }
    });
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
