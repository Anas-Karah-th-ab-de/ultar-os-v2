//! `ultar-updated` – the update service of Ultar OS.
//!
//! ```text
//! ultar-updated --config FILE [--socket PATH] [--lang ar|en|de]
//! ```
//! The socket may also come from `ULTAR_BUS_SOCKET`. The service ends when the bus connection
//! ends.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use ultar_i18n::{Arg, Lang};
use ultar_update::daemon::{parse_args, run, SOCKET_ENV};
use ultar_update::msg::{lang_hint, translator};

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let tr = translator(lang_hint(&words).unwrap_or_else(Lang::from_env));
    let outcome = parse_args(&words, std::env::var(SOCKET_ENV).ok())
        .and_then(|args| run(&args, &mut |_| eprintln!("{}", tr.t("update.daemon.ready"))));
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let (line, detail) = error.detail();
            let args = [
                ("line", Arg::Int(line as i64)),
                ("detail", Arg::Str(&detail)),
            ];
            eprintln!("{}", tr.t_args(error.key(), &args));
            ExitCode::from(if error.key() == "update.daemon.usage" {
                2
            } else {
                1
            })
        }
    }
}
