//! `ultar-sysdialog` – the system dialogs of Ultar OS (permission, administrator
//! password, "keep these display settings?").
//!
//! ```text
//! ultar-sysdialog [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]
//! ```
//! Runs as its own user with the compositor right `ui.system-dialog`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_desktop::app::{parse_options, sysdialog};
use ultar_service::{exit, Runtime, ServiceSpec};

fn main() -> ExitCode {
    let Some((options, rest)) = parse_options(std::env::args().skip(1)) else {
        return ExitCode::from(exit::USAGE as u8);
    };
    if !rest.is_empty() {
        return ExitCode::from(exit::USAGE as u8);
    }
    let Ok(runtime) = Runtime::start(ServiceSpec::new("ultar-sysdialog")) else {
        return ExitCode::from(exit::HARDENING as u8);
    };
    let code = runtime.run(|rt| {
        rt.ready();
        let stop = || rt.lifecycle().is_stopping();
        match sysdialog::run(&options, &stop) {
            Ok(()) => exit::OK,
            Err(_) => exit::FAILURE,
        }
    });
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
