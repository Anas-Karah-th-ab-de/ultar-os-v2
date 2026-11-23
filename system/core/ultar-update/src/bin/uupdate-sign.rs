//! `uupdate-sign` – the publisher's tool: keys, manifests, signatures, rotations, bundles.
//! Secret keys are read from files only. Usage: run without arguments.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use ultar_i18n::Lang;
use ultar_update::msg::{lang_hint, translator};

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let tr = translator(lang_hint(&words).unwrap_or_else(Lang::from_env));
    ExitCode::from(ultar_update::signcli::run(
        &words,
        &tr,
        &mut std::io::stdout(),
    ))
}
