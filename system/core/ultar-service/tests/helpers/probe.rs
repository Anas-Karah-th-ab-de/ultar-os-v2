//! `ultar-service-probe` – test helper: the minimal daemon the runtime's
//! integration tests start as a real process. It also shows what the
//! runtime achieves on the running kernel when started by hand.
//!
//! ```text
//! ultar-service-probe [--allow-root] [--no-limits] [--nofile N] [--deadline-ms N]
//!                     [--secret-fd N | --secret-file PATH] [--log-socket PATH]
//!                     [--hang] [--panic]
//! ```
//!
//! It applies the default hardening, prints one `key=value` line per fact
//! to standard output (machine readable, no translation: this is a
//! diagnostic tool), reports `ready`, then waits for a stop signal.
//! `--hang` ignores the stop (to see the deadline), `--panic` panics after
//! becoming ready (to see the panic hook).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use ultar_service::log::{LogTarget, LoggerConfig};
use ultar_service::{
    current_limit, exit, read_secret_fd, read_secret_file, Resource, Runtime, SecretFilePolicy,
    ServiceSpec, State,
};

#[derive(Default)]
struct Options {
    allow_root: bool,
    no_limits: bool,
    nofile: Option<u64>,
    deadline_ms: Option<u64>,
    secret_fd: Option<i32>,
    secret_file: Option<PathBuf>,
    log_socket: Option<PathBuf>,
    hang: bool,
    panic: bool,
}

fn parse(mut args: impl Iterator<Item = String>) -> Option<Options> {
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--allow-root" => options.allow_root = true,
            "--no-limits" => options.no_limits = true,
            "--hang" => options.hang = true,
            "--panic" => options.panic = true,
            "--nofile" => options.nofile = Some(args.next()?.parse().ok()?),
            "--deadline-ms" => options.deadline_ms = Some(args.next()?.parse().ok()?),
            "--secret-fd" => options.secret_fd = Some(args.next()?.parse().ok()?),
            "--secret-file" => options.secret_file = Some(PathBuf::from(args.next()?)),
            "--log-socket" => options.log_socket = Some(PathBuf::from(args.next()?)),
            _ => return None,
        }
    }
    Some(options)
}

fn say(line: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn spec(options: &Options) -> ServiceSpec {
    let mut spec = ServiceSpec::new("probe");
    spec.hardening.allow_root = options.allow_root;
    if options.no_limits {
        spec.hardening.limits = None;
    }
    if let (Some(nofile), Some(limits)) = (options.nofile, spec.hardening.limits.as_mut()) {
        limits.open_files = Some(nofile);
    }
    if let Some(fd) = options.secret_fd {
        spec.hardening.keep_descriptors.push(fd);
    }
    if let Some(ms) = options.deadline_ms {
        spec.stop_deadline = Duration::from_millis(ms);
    }
    if let Some(path) = &options.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    spec
}

fn report(runtime: &Runtime, options: &Options) {
    let hardening = runtime.hardening();
    say(&format!("platform={:?}", hardening.platform));
    say(&format!("closed_fds={}", hardening.closed_descriptors));
    for (step, outcome) in &hardening.steps {
        say(&format!("step.{}={outcome:?}", step.key()));
    }
    let limits = [
        ("as", Resource::AddressSpace),
        ("nofile", Resource::OpenFiles),
        ("nproc", Resource::Processes),
        ("core", Resource::Core),
    ];
    for (name, resource) in limits {
        if let Ok((soft, hard)) = current_limit(resource) {
            say(&format!("limit.{name}={soft}/{hard}"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        say(&format!("cwd={}", cwd.display()));
    }
    let secret = match (&options.secret_fd, &options.secret_file) {
        (Some(fd), _) => Some(read_secret_fd(*fd)),
        (None, Some(path)) => Some(read_secret_file(
            path,
            &SecretFilePolicy::for_current_user(),
        )),
        (None, None) => None,
    };
    match secret {
        // Only the length: the probe must not print a secret either.
        Some(Ok(secret)) => say(&format!("secret.len={}", secret.len())),
        Some(Err(error)) => say(&format!("secret.error={}", error.key())),
        None => {}
    }
}

fn main() -> ExitCode {
    let Some(options) = parse(std::env::args().skip(1)) else {
        let _ = writeln!(std::io::stderr(), "probe.usage");
        return exit::code(exit::USAGE);
    };
    let runtime = match Runtime::start(spec(&options)) {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{}", error.key());
            return exit::code(error.exit_code());
        }
    };
    exit::code(runtime.run(|runtime| {
        report(runtime, &options);
        runtime.lifecycle().on_reload(|| say("reload"));
        runtime.ready();
        say("ready");
        if options.panic {
            // Deliberate: demonstrates the panic hook of the runtime.
            #[allow(clippy::panic)]
            {
                panic!("probe panic with a secret-looking text that must not be logged");
            }
        }
        let cause = runtime.lifecycle().wait_stop();
        say(&format!("stopping={}", cause.name()));
        if options.hang {
            // Never reaches `Stopped`: the deadline watchdog ends the process.
            runtime.lifecycle().wait_for(State::Stopped);
        }
        exit::OK
    }))
}
