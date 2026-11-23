//! `ulog` – read, follow, verify and export the log of Ultar OS.
//! See [`ultar_logd::msg::ULOG_SYNOPSIS`].

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::process::ExitCode;
use ultar_logd::cli::{parse, run, Streams, EXIT_USAGE};
use ultar_logd::msg::{language, text, Msg, ULOG_SYNOPSIS};

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let lang = language(&words);
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let code = match parse(words) {
        Ok(args) => {
            let mut streams = Streams {
                out: &mut out,
                err: &mut err,
                lang,
            };
            run(&args, &mut streams)
        }
        Err(_) => {
            let _ = writeln!(err, "{}", text(lang, Msg::BadArguments));
            let _ = writeln!(err, "{}\n  {ULOG_SYNOPSIS}", text(lang, Msg::Usage));
            EXIT_USAGE
        }
    };
    let _ = out.flush();
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
