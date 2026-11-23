//! `ultar-updated` – the update service of Ultar OS.
//!
//! ```text
//! ultar-updated --config FILE [--socket PATH] [--log-socket PATH] [--lang ar|en|de]
//! ```
//! The socket may also come from `ULTAR_BUS_SOCKET`.
//!
//! Runs under the service runtime (`ultar-service`): at start it closes inherited descriptors,
//! sets umask 077, changes to `/`, sets the standard limits, drops every kernel right and forbids
//! privilege gain; it runs as root on purpose (it writes the slots and the boot control). It
//! reports readiness to `ultar-init` once it is registered on the bus. `SIGTERM`/`SIGINT` end
//! it: a running operation stops at its next checkpoint without clean-up (every point is safe,
//! as for a power loss), the bus connection is closed, exit code 0.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use ultar_i18n::{Arg, Lang, Translator};
use ultar_service::log::{LogTarget, LoggerConfig};
use ultar_service::{exit, Runtime, ServiceSpec};
use ultar_update::daemon::{parse_args, run, DaemonArgs, DaemonError, Hooks, SOCKET_ENV};
use ultar_update::msg::{lang_hint, translator};

fn report(tr: &Translator, error: &DaemonError) -> i32 {
    let (line, detail) = error.detail();
    let args = [
        ("line", Arg::Int(line as i64)),
        ("detail", Arg::Str(&detail)),
    ];
    eprintln!("{}", tr.t_args(error.key(), &args));
    if matches!(error, DaemonError::Usage) {
        exit::USAGE
    } else {
        exit::FAILURE
    }
}

fn start_runtime(args: &DaemonArgs, tr: &Translator) -> Result<Runtime, ExitCode> {
    let mut spec = ServiceSpec::new("ultar-updated");
    // Slots, boot control and the state directory belong to root.
    spec.hardening.allow_root = true;
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    Runtime::start(spec).map_err(|error| {
        let detail = error.to_string();
        let message = tr.t_args(
            "update.daemon.hardening_failed",
            &[("detail", Arg::Str(&detail))],
        );
        eprintln!("{message}");
        exit::code(error.exit_code())
    })
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let tr = translator(lang_hint(&words).unwrap_or_else(Lang::from_env));
    let args = match parse_args(&words, std::env::var(SOCKET_ENV).ok()) {
        Ok(args) => args,
        Err(error) => return exit::code(report(&tr, &error)),
    };
    // Before anything is opened and before the first thread exists.
    let runtime = match start_runtime(&args, &tr) {
        Ok(runtime) => runtime,
        Err(code) => return code,
    };
    let lang = tr.lang();
    exit::code(runtime.run(|runtime| serve(&args, lang, runtime)))
}

/// Everything after the runtime is up; runs on the work thread.
fn serve(args: &DaemonArgs, lang: Lang, runtime: &Runtime) -> i32 {
    let tr = translator(lang);
    let mut ready = |_| {
        runtime.ready();
        eprintln!("{}", tr.t("update.daemon.ready"));
    };
    let on_stop = |action: Box<dyn FnOnce() + Send>| runtime.lifecycle().on_stop(action);
    let hooks = Hooks {
        ready: &mut ready,
        on_stop: &on_stop,
    };
    match run(args, hooks) {
        Ok(()) => exit::OK,
        Err(error) => report(&tr, &error),
    }
}
