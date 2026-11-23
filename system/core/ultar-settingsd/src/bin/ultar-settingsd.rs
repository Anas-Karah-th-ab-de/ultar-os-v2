//! `ultar-settingsd` – the settings service of Ultar OS.
//!
//! ```text
//! ultar-settingsd --store DIR [--socket PATH] [--user NAME]
//!                 [--policy FILE | --allow-all] [--profile FILE]
//!                 [--write-delay-ms N] [--token-file FILE | --token-fd N]
//!                 [--log-socket PATH] [--allow-root] [--log-apply] [--lang ar|en|de]
//! ```
//!
//! The socket may also come from `ULTAR_BUS_SOCKET`. `SIGTERM`/`SIGINT`
//! (or a `settings.shutdown` call from an administrator) end the service
//! after everything pending is written to disk; the exit code is 0, also
//! when the stop comes while the broker is still awaited. `SIGHUP` reads
//! the policy file again (a faulty file keeps the old policy).
//! `--token-fd N` must name a descriptor the service manager passed (a pipe
//! or a file); any other number stops the start.
//!
//! The service limits itself at start (`ultar_service::harden`): default
//! resource limits, umask 077, no undeclared inherited descriptors, no kernel rights,
//! and no root unless `--allow-root` declares it.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::{stderr, stdout};
use std::process::ExitCode;
use ultar_i18n::{Arg, Lang, Translator};
use ultar_service::log::{LogTarget, LoggerConfig};
use ultar_service::{exit, HardeningError, Runtime, ServiceSpec, StartError};
use ultar_settingsd::cli::{lang_hint, SOCKET_ENV};
use ultar_settingsd::daemon::{parse, run, DaemonArgs, DaemonCommand, Hooks};
use ultar_settingsd::i18n;

/// Largest bus token read from a file or descriptor.
const MAX_TOKEN_LEN: usize = 4096;

fn translator(lang: Option<Lang>) -> Translator {
    let lang = lang.unwrap_or_else(Lang::from_env);
    // Without catalogs the keys themselves are printed.
    i18n::translator(lang).unwrap_or_else(|_| Translator::empty(lang))
}

fn start_runtime(args: &DaemonArgs, translator: &Translator) -> Result<Runtime, ExitCode> {
    let mut spec = ServiceSpec::new("ultar-settingsd");
    spec.hardening.allow_root = args.allow_root;
    if let Some(fd) = args.token_fd {
        spec.inherited.push(fd);
    }
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    Runtime::start(spec).map_err(|error| {
        let message = match error {
            StartError::Hardening(HardeningError::RunningAsRoot) => {
                translator.t("settingsd.daemon.root_refused").into_owned()
            }
            other => {
                let detail = other.to_string();
                translator.t_args(
                    "settingsd.daemon.hardening_failed",
                    &[("detail", Arg::Str(&detail))],
                )
            }
        };
        eprintln!("{message}");
        exit::code(error.exit_code())
    })
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
            return exit::code(exit::USAGE);
        }
    };
    let translator = translator(args.lang);
    // Before anything is opened and before the first thread exists.
    let runtime = match start_runtime(&args, &translator) {
        Ok(runtime) => runtime,
        Err(code) => return code,
    };
    // The work thread makes its own translator and streams.
    let lang = translator.lang();
    exit::code(runtime.run(|runtime| serve(&args, lang, runtime)))
}

/// Everything after the runtime is up; runs on the work thread.
fn serve(args: &DaemonArgs, lang: Lang, runtime: &Runtime) -> i32 {
    let translator = translator(Some(lang));
    let hooks = Hooks {
        lifecycle: runtime.lifecycle(),
        logger: runtime.logger(),
        ready: &|| runtime.ready(),
        secret: &|source| runtime.read_secret(source, MAX_TOKEN_LEN),
    };
    match run(args, &translator, &mut stdout(), &mut stderr(), &hooks) {
        Ok(()) => exit::OK,
        Err(error) => {
            let detail = error.detail();
            let message = translator.t_args(error.message_key(), &[("detail", Arg::Str(&detail))]);
            eprintln!("{message}");
            exit::FAILURE
        }
    }
}
