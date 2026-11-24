//! `ultar-lock` – the lock screen of Ultar OS (stays connected all session).
//!
//! ```text
//! ultar-lock [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light] [--user NAME]
//! ```
//! Runs as its own user with the compositor right `ui.lock-screen`. `ultar-init` restarts
//! it at once when it ends (`essential = true`); the session stays locked meanwhile.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_desktop::app::{lock, parse_options};
use ultar_service::{exit, Runtime, ServiceSpec};

fn main() -> ExitCode {
    let Some((options, rest)) = parse_options(std::env::args().skip(1)) else {
        return ExitCode::from(exit::USAGE as u8);
    };
    let user = match rest.as_slice() {
        [flag, name] if flag == "--user" => Some(name.clone()),
        [] => None,
        _ => return ExitCode::from(exit::USAGE as u8),
    };
    let Ok(runtime) = Runtime::start(ServiceSpec::new("ultar-lock")) else {
        return ExitCode::from(exit::HARDENING as u8);
    };
    let code = runtime.run(|rt| {
        rt.ready();
        let stop = || rt.lifecycle().is_stopping();
        match lock::run(&options, user.clone(), &stop) {
            Ok(()) => exit::OK,
            Err(_) => exit::FAILURE,
        }
    });
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
