//! `ultar-settingsd` – the settings service of Ultar OS.
//!
//! ```text
//! ultar-settingsd --store DIR [--socket PATH] [--user NAME]
//!                 [--policy FILE | --allow-all] [--profile FILE]
//!                 [--write-delay-ms N] [--token TOKEN] [--log-apply] [--lang ar|en|de]
//! ```
//!
//! The socket may also come from `ULTAR_BUS_SOCKET`. The service ends on a
//! `settings.shutdown` call from an administrator.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::{stderr, stdout};
use std::process::ExitCode;
use ultar_i18n::{Arg, Lang, Translator};
use ultar_settingsd::cli::{lang_hint, SOCKET_ENV};
use ultar_settingsd::daemon::{parse, run, DaemonCommand};
use ultar_settingsd::i18n;

fn translator(lang: Option<Lang>) -> Translator {
    let lang = lang.unwrap_or_else(Lang::from_env);
    // Without catalogs the keys themselves are printed.
    i18n::translator(lang).unwrap_or_else(|_| Translator::empty(lang))
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let command = parse(words.iter().cloned(), std::env::var(SOCKET_ENV).ok());
    let args = match command {
        Ok(DaemonCommand::Run(args)) => args,
        Ok(DaemonCommand::Help) => {
            let translator = translator(lang_hint(&words));
            println!("{}", translator.t("settingsd.daemon.usage"));
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            let translator = translator(lang_hint(&words));
            eprintln!("{}", translator.t(error.message_key()));
            eprintln!("{}", translator.t("settingsd.daemon.usage"));
            return ExitCode::from(2);
        }
    };
    let translator = translator(args.lang);
    match run(&args, &translator, &mut stdout(), &mut stderr()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let detail = error.detail();
            let message = translator.t_args(error.message_key(), &[("detail", Arg::Str(&detail))]);
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
