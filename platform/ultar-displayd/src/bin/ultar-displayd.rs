//! `ultar-displayd` – the display service (see the library documentation).
//!
//! ```text
//! ultar-displayd --bus PATH --compositor PATH [--log-socket PATH] [--token-file FILE] [--allow-root]
//! ```
//! The bus socket may also come from `ULTAR_BUS_SOCKET`. `SIGTERM` ends it
//! (exit 0); its compositor connection ends with it, which takes a setup
//! waiting for "Keep?" back at once.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_displayd::daemon::{parse, run, SOCKET_ENV};
use ultar_displayd::msg::{text, Lang, Msg};
use ultar_service::log::{LogTarget, LoggerConfig};
use ultar_service::{exit, Runtime, SecretFilePolicy, ServiceSpec};

/// Largest bus token read from a file.
const MAX_TOKEN: usize = 4096;

fn main() -> ExitCode {
    let lang = Lang::from_tag(&std::env::var("LANG").unwrap_or_default());
    let args = match parse(std::env::args().skip(1), std::env::var(SOCKET_ENV).ok()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{}", text(lang, Msg::Usage));
            return ExitCode::SUCCESS;
        }
        Err(_) => {
            eprintln!(
                "{}\n{}",
                text(lang, Msg::BadArguments),
                text(lang, Msg::Usage)
            );
            return exit::code(exit::USAGE);
        }
    };
    let mut spec = ServiceSpec::new("ultar-displayd");
    spec.hardening.allow_root = args.allow_root;
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    let runtime = match Runtime::start(spec) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", text(lang, Msg::Hardening));
            return exit::code(e.exit_code());
        }
    };
    exit::code(runtime.run(|rt| {
        let token = match &args.token_file {
            Some(path) => {
                match ultar_service::read_secret_file_limited(
                    path,
                    &SecretFilePolicy::for_current_user(),
                    MAX_TOKEN,
                ) {
                    Ok(t) => Some(t),
                    Err(_) => {
                        eprintln!("{}", text(lang, Msg::BadArguments));
                        return exit::USAGE;
                    }
                }
            }
            None => None,
        };
        match run(&args, token, rt.lifecycle(), rt.logger(), &|| rt.ready()) {
            Ok(()) => exit::OK,
            Err(_) if rt.lifecycle().is_stopping() => exit::OK,
            Err(_) => {
                eprintln!("{}", text(lang, Msg::Bus));
                exit::FAILURE
            }
        }
    }))
}
