//! `ultar-desktop` – the bar of Ultar OS with its sheets, overview and Super+P.
//!
//! ```text
//! ultar-desktop [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light] [--user NAME]
//! ```
//! Runs as its own user with the compositor right `ui.shell`; `SIGTERM` ends it.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_desktop::app::{desktop, parse_options};
use ultar_service::{exit, Runtime, ServiceSpec};

fn main() -> ExitCode {
    let Some((options, rest)) = parse_options(std::env::args().skip(1)) else {
        return ExitCode::from(exit::USAGE as u8);
    };
    let user = match rest.as_slice() {
        [flag, name] if flag == "--user" => name.clone(),
        [] => std::env::var("USER").unwrap_or_default(),
        _ => return ExitCode::from(exit::USAGE as u8),
    };
    let Ok(runtime) = Runtime::start(ServiceSpec::new("ultar-desktop")) else {
        return ExitCode::from(exit::HARDENING as u8);
    };
    let code = runtime.run(|rt| {
        rt.ready();
        let stop = || rt.lifecycle().is_stopping();
        match desktop::run(&options, &user, &stop) {
            Ok(()) => exit::OK,
            Err(_) => exit::FAILURE,
        }
    });
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
