//! `usettings` – read and change Ultar OS settings from the command line.
//!
//! ```text
//! usettings [--socket PATH] [--lang ar|en|de] [--token-file FILE]
//!           get KEY | set KEY VALUE | reset KEY | list [PANEL]
//!           | watch [--count N] | confirm [KEY] | revert [KEY]
//! ```
//!
//! Exit status: 0 done, 1 the service refused or is not reachable, 2 the
//! command line was not understood.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::stdout;
use std::process::ExitCode;
use ultar_bus::{Client, ClientOptions};
use ultar_i18n::{Arg, Lang, Translator};
use ultar_service::{read_secret_file, SecretFilePolicy};
use ultar_settingsd::cli::{
    lang_hint, parse, CliError, Command, Invocation, Session, UsageError, SOCKET_ENV,
};
use ultar_settingsd::{i18n, seed};

fn translator(lang: Option<Lang>) -> Translator {
    let lang = lang.unwrap_or_else(Lang::from_env);
    // Without catalogs the keys themselves are printed.
    i18n::translator(lang).unwrap_or_else(|_| Translator::empty(lang))
}

fn usage_failure(translator: &Translator, message: &str) -> ExitCode {
    eprintln!("{message}");
    eprintln!("{}", translator.t("usettings.usage"));
    ExitCode::from(2)
}

fn fail(translator: &Translator, error: &CliError) -> ExitCode {
    eprintln!("{}", translator.t(error.message_key()));
    ExitCode::FAILURE
}

/// The bus token from the file named on the command line, if any.
fn load_token(
    invocation: &Invocation,
    translator: &Translator,
) -> Result<Option<String>, ExitCode> {
    let Some(path) = &invocation.token_file else {
        return Ok(None);
    };
    match read_secret_file(path, &SecretFilePolicy::for_current_user()) {
        Ok(secret) => Ok(secret.expose_str().map(str::to_owned)),
        Err(error) => {
            let detail = error.to_string();
            let args = [("detail", Arg::Str(&detail))];
            eprintln!("{}", translator.t_args("usettings.error.token_file", &args));
            Err(ExitCode::FAILURE)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match parse(args.iter().cloned(), std::env::var(SOCKET_ENV).ok()) {
        Ok(invocation) => invocation,
        Err(error) => {
            let translator = translator(lang_hint(&args));
            let message = match &error {
                UsageError::Arguments => translator.t("usettings.error.arguments").into_owned(),
                UsageError::UnknownOption(option) => translator.t_args(
                    "usettings.error.unknown_option",
                    &[("detail", Arg::Str(option))],
                ),
            };
            return usage_failure(&translator, &message);
        }
    };
    let translator = translator(invocation.lang);
    if invocation.command == Command::Help {
        println!("{}", translator.t("usettings.usage"));
        return ExitCode::SUCCESS;
    }
    let Some(socket) = &invocation.socket else {
        let message = translator.t("usettings.error.no_socket").into_owned();
        return usage_failure(&translator, &message);
    };
    let Ok(registry) = seed::registry() else {
        return fail(&translator, &CliError::UnknownKey);
    };
    let token = match load_token(&invocation, &translator) {
        Ok(token) => token,
        Err(code) => return code,
    };
    let options = ClientOptions {
        token,
        reconnect: false,
        ..ClientOptions::named("usettings")
    };
    let bus = match Client::connect(socket, options) {
        Ok(bus) => bus,
        Err(error) => return fail(&translator, &CliError::Bus(error)),
    };
    let mut out = stdout();
    let mut session = Session {
        bus: &bus,
        registry: &registry,
        translator: &translator,
        out: &mut out,
    };
    match session.run(&invocation.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&translator, &error),
    }
}
