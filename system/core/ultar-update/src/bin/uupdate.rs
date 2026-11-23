//! `uupdate` – command-line client of the update service. Usage: run without arguments.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use ultar_i18n::Lang;
use ultar_update::daemon::SOCKET_ENV;
use ultar_update::msg::{lang_hint, translator};

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let tr = translator(lang_hint(&words).unwrap_or_else(Lang::from_env));
    let socket = std::env::var(SOCKET_ENV).ok();
    ExitCode::from(ultar_update::cli::run(
        &words,
        socket,
        &tr,
        &mut std::io::stdout(),
    ))
}
