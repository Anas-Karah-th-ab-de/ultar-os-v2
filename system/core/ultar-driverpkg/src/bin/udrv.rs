//! `udrv` – create, sign, verify and index Ultar driver packages (`.utr`).

#![forbid(unsafe_code)]

use std::process::ExitCode;
use ultar_driverpkg::cli::{describe, run, CliError};
use ultar_driverpkg::msg::{text, Lang};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lang = Lang::from_env();
    match run(&args, lang, &mut std::io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", describe(&error, lang));
            if matches!(error, CliError::Usage | CliError::UnknownCommand(_)) && !args.is_empty() {
                eprintln!("{}", text(lang, "driverpkg.cli.usage").unwrap_or_default());
            }
            ExitCode::FAILURE
        }
    }
}
