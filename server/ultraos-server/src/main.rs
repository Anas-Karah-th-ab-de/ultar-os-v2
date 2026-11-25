//! ultraos-server binary: `ultraos-server` runs the service, `ultraos-server admin …` manages
//! entitlements, channels and publications on the same database. Configuration comes from the
//! environment (see DEPLOY.md).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use rand::Rng;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use ultraos_server::clock::SystemClock;
use ultraos_server::guard::ProxyNet;
use ultraos_server::msg::{self, Lang};
use ultraos_server::ratelimit::BucketConfig;
use ultraos_server::{cli, db::Db, device_auth, files::FileStore, AppState, Config};

/// Log writer that never reports failure. If stderr is closed or full, log lines are dropped
/// instead of turning into errors inside request handling.
struct LossyStderr;

impl std::io::Write for LossyStderr {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn fail(lang: Lang, key: &str, detail: String) -> ExitCode {
    eprintln!("{}", msg::render(lang, key, &[detail]));
    ExitCode::from(1)
}

/// Token key from `ULTRAOS_SECRET` (64 hex characters). Unset: a random key, so tokens do not
/// survive a restart. Set but malformed: `None` (the server refuses to start).
fn secret_from_env() -> Option<[u8; 32]> {
    match std::env::var("ULTRAOS_SECRET") {
        Ok(text) => {
            let bytes = hex::decode(text.trim()).ok()?;
            <[u8; 32]>::try_from(bytes.as_slice()).ok()
        }
        Err(_) => {
            let mut key = [0u8; 32];
            rand::thread_rng().fill(&mut key);
            tracing::warn!(event = "random_token_key", note = "ULTRAOS_SECRET not set");
            Some(key)
        }
    }
}

/// Reads a limit of the form `burst,per_minute` (for example `120,120`); a malformed value is
/// reported and the default stays in force.
fn limit_from_env(key: &str, bucket: &mut BucketConfig) {
    let Ok(text) = std::env::var(key) else { return };
    let parsed = text.split_once(',').and_then(|(burst, rate)| {
        let capacity = burst.trim().parse::<u32>().ok().filter(|n| *n > 0)?;
        let refill_per_minute = rate.trim().parse::<u32>().ok().filter(|n| *n > 0)?;
        Some(BucketConfig {
            capacity,
            refill_per_minute,
        })
    });
    match parsed {
        Some(value) => *bucket = value,
        None => tracing::warn!(event = "ignored_limit", key = %key),
    }
}

/// Device identity settings; a malformed value is reported and the default stays in force.
fn devices_from_env(devices: &mut device_auth::DeviceConfig) {
    fn apply<T>(key: &str, parse: impl Fn(&str) -> Option<T>, target: &mut T) {
        let Ok(text) = std::env::var(key) else { return };
        match parse(&text) {
            Some(value) => *target = value,
            None => tracing::warn!(event = "ignored_setting", key = %key),
        }
    }
    apply(
        "ULTRAOS_SERVER_NAME",
        device_auth::normalize_server_name,
        &mut devices.server_name,
    );
    apply(
        "ULTRAOS_MAX_DEVICES",
        device_auth::parse_max_devices,
        &mut devices.max_devices,
    );
    apply(
        "ULTRAOS_PASSWORD_FAILURES",
        device_auth::parse_password_failures,
        &mut devices.password_failures,
    );
    apply(
        "ULTRAOS_DEVICE_COOLDOWN_SECS",
        device_auth::parse_cooldown,
        &mut devices.recovery_cooldown_secs,
    );
}

fn config_from_env(secret: [u8; 32]) -> Config {
    let mut config = Config::new(
        env_or("ULTRAOS_DB", "ultraos.sqlite"),
        env_or("ULTRAOS_FILES", "ultraos-files"),
        secret,
    );
    config.wg_server_pubkey = env_or("ULTRAOS_WG_PUBKEY", "<wg-server-pubkey>");
    config.wg_endpoint = env_or("ULTRAOS_WG_ENDPOINT", "ultar.code-craft.tech:51820");
    config.turn_secret = env_or("ULTRAOS_TURN_SECRET", "");
    config.turn_host = env_or("ULTRAOS_TURN_HOST", "ultar.code-craft.tech");
    if let Ok(list) = std::env::var("ULTRAOS_TRUSTED_PROXIES") {
        config.trusted_proxies = Vec::new();
        for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match ProxyNet::parse(part) {
                Some(net) => config.trusted_proxies.push(net),
                None => tracing::warn!(event = "ignored_trusted_proxy", value = %part),
            }
        }
    }
    limit_from_env("ULTRAOS_LIMIT_GENERAL", &mut config.limits.general);
    limit_from_env("ULTRAOS_LIMIT_DOWNLOAD", &mut config.limits.download);
    limit_from_env("ULTRAOS_LIMIT_AUTH", &mut config.limits.auth);
    devices_from_env(&mut config.devices);
    if let Ok(text) = std::env::var("ULTRAOS_MAX_CONNECTIONS_PER_PEER") {
        match ultraos_server::server::parse_max_connections_per_peer(&text) {
            Some(n) => config.max_connections_per_peer = n,
            None => tracing::warn!(
                event = "ignored_setting",
                key = "ULTRAOS_MAX_CONNECTIONS_PER_PEER"
            ),
        }
    }
    if let Ok(text) = std::env::var("ULTRAOS_MAX_CONNECTIONS") {
        match ultraos_server::server::parse_max_connections(&text) {
            Some(n) => config.max_connections = n,
            None => tracing::warn!(event = "ignored_setting", key = "ULTRAOS_MAX_CONNECTIONS"),
        }
    }
    config
}

fn admin(lang: Lang, args: &[String]) -> ExitCode {
    let db = match Db::open(&env_or("ULTRAOS_DB", "ultraos.sqlite")) {
        Ok(db) => db,
        Err(e) => return fail(lang, "server.error.database", e.to_string()),
    };
    let files = FileStore::new(env_or("ULTRAOS_FILES", "ultraos-files"));
    match cli::run(args, &db, &files) {
        Ok(outcome) => {
            println!("{}", msg::render(lang, outcome.key, &outcome.args));
            for line in outcome.lines {
                println!("  {line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", msg::render(lang, e.message_key(), &[e.detail()]));
            ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1))
        }
    }
}

async fn run_server(lang: Lang) -> ExitCode {
    let Some(secret) = secret_from_env() else {
        return fail(lang, "server.error.secret", String::new());
    };
    let state = match AppState::new(config_from_env(secret), Arc::new(SystemClock::new())) {
        Ok(state) => state,
        Err(e) => return fail(lang, "server.error.database", e.to_string()),
    };
    let addr = env_or("ULTRAOS_BIND", "127.0.0.1:8080");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => return fail(lang, "server.error.bind", format!("{addr}: {e}")),
    };
    // Forget idle client keys even when no request arrives (see ratelimit module).
    let sweeper = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tick.tick().await;
            sweeper.limiter.sweep();
        }
    });
    let bound = listener.local_addr().map(|a| a.to_string()).unwrap_or(addr);
    tracing::info!(event = "listening", addr = %bound);
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match ultraos_server::serve(listener, state, shutdown).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(lang, "server.error.serve", e.to_string()),
    }
}

/// Log level from `ULTRAOS_LOG` (`error`, `warn`, `info`, `debug`, `off`; default `info`).
/// A release server runs with `warn`: no line per request, no account or device events.
fn log_level() -> Option<tracing::Level> {
    match std::env::var("ULTRAOS_LOG").as_deref() {
        Ok("off") => None,
        Ok("error") => Some(tracing::Level::ERROR),
        Ok("warn") => Some(tracing::Level::WARN),
        Ok("debug") => Some(tracing::Level::DEBUG),
        _ => Some(tracing::Level::INFO),
    }
}

fn main() -> ExitCode {
    // Logs go to stderr without colours (journald); stdout is for command results only.
    let Some(level) = log_level() else {
        return run_main();
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .with_ansi(false)
        .with_writer(|| LossyStderr)
        .init();
    run_main()
}

fn run_main() -> ExitCode {
    let lang = Lang::from_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("admin") => admin(lang, &args[1..]),
        Some(_) => {
            eprintln!("{}", msg::render(lang, "admin.usage", &[]));
            ExitCode::from(2)
        }
        None => match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(run_server(lang)),
            Err(e) => fail(lang, "server.error.runtime", e.to_string()),
        },
    }
}
