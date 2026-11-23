//! Command line of `ultar-logd`.

use crate::core::CoreConfig;
use crate::store::FsyncPolicy;
use std::path::PathBuf;
use ultar_service::log::RateLimit;
use ultar_service::SecretSource;

/// One ingestion socket as named on the command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenArg {
    /// Path of the socket file.
    pub path: PathBuf,
    /// Fixed source name; `None`: kernel credentials through the uid map.
    pub name: Option<String>,
    /// Permission bits of the socket file.
    pub mode: Option<u32>,
}

/// The parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonArgs {
    /// Directory of the segment files.
    pub dir: PathBuf,
    /// Ingestion sockets.
    pub listeners: Vec<ListenArg>,
    /// File mapping user ids to identities.
    pub uid_map: Option<PathBuf>,
    /// Bus socket for the reading service `log`.
    pub bus: Option<PathBuf>,
    /// Token for a bus listener that authenticates by token: a checked
    /// file (`--bus-token-file`) or an inherited descriptor
    /// (`--bus-token-fd`, for kernels whose files cannot be checked).
    pub bus_token: Option<SecretSource>,
    /// Bus policy file that decides who has `log.read_all`.
    pub policy: Option<PathBuf>,
    /// Everybody may read everything (development).
    pub allow_all: bool,
    /// Secret key of the hash chain (`--chain-key-file` or
    /// `--chain-key-fd`).
    pub chain_key: Option<SecretSource>,
    /// The kernel's log file.
    pub kernel_log: Option<PathBuf>,
    /// Limits.
    pub config: CoreConfig,
    /// The operator declares that the service runs as root.
    pub allow_root: bool,
}

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonCommand {
    /// Print the usage.
    Help,
    /// Run the service.
    Run(Box<DaemonArgs>),
}

/// Why the command line was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgError {
    /// Missing value, bad number, unknown option, no directory or socket.
    Arguments,
    /// `--bus` without `--policy FILE` or `--allow-all`.
    NoPolicy,
    /// `--policy` together with `--allow-all`.
    PolicyConflict,
    /// A credential listener without `--uid-map`.
    NoUidMap,
    /// Limits that do not fit together (see `StoreConfig::is_valid`).
    Limits,
}

fn listen(spec: &str) -> Result<ListenArg, ArgError> {
    let (path, name) = match spec.rsplit_once('=') {
        Some((path, name)) => (path, Some(name)),
        None => (spec, None),
    };
    if path.is_empty() || name.is_some_and(|name| !crate::record::is_service_name(name)) {
        return Err(ArgError::Arguments);
    }
    Ok(ListenArg {
        path: PathBuf::from(path),
        name: name.map(str::to_owned),
        mode: None,
    })
}

fn mode(text: &str) -> Result<u32, ArgError> {
    let valid =
        !text.is_empty() && text.len() <= 4 && text.bytes().all(|b| (b'0'..=b'7').contains(&b));
    match u32::from_str_radix(text, 8) {
        Ok(mode) if valid && mode <= 0o777 => Ok(mode),
        _ => Err(ArgError::Arguments),
    }
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, ArgError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ArgError::Arguments);
    }
    text.parse().map_err(|_| ArgError::Arguments)
}

fn fsync(text: &str, current: FsyncPolicy) -> Result<FsyncPolicy, ArgError> {
    match text {
        "always" => Ok(FsyncPolicy::Always),
        "rotate" => Ok(FsyncPolicy::OnRotate),
        "batch" => Ok(match current {
            FsyncPolicy::Batch(count) => FsyncPolicy::Batch(count),
            _ => FsyncPolicy::Batch(64),
        }),
        _ => Err(ArgError::Arguments),
    }
}

#[derive(Default)]
struct Raw {
    dir: Option<PathBuf>,
    listeners: Vec<ListenArg>,
    uid_map: Option<PathBuf>,
    bus: Option<PathBuf>,
    bus_token: Option<SecretSource>,
    policy: Option<PathBuf>,
    chain_key: Option<SecretSource>,
    kernel_log: Option<PathBuf>,
    allow_all: bool,
    allow_root: bool,
    help: bool,
}

fn path_option(raw: &mut Raw, name: &str, value: String) -> bool {
    let slot = match name {
        "--dir" => &mut raw.dir,
        "--uid-map" => &mut raw.uid_map,
        "--bus" => &mut raw.bus,
        "--policy" => &mut raw.policy,
        "--kernel-log" => &mut raw.kernel_log,
        _ => return false,
    };
    *slot = Some(PathBuf::from(value));
    true
}

/// Lowest descriptor number a secret may be passed on (0 to 2 are the
/// standard streams).
const FIRST_SECRET_FD: i32 = 3;

/// `--bus-token-file|fd`, `--chain-key-file|fd`: each secret has one
/// source; naming it twice is an error. `Ok(false)`: not such an option.
fn secret_option(raw: &mut Raw, name: &str, value: &str) -> Result<bool, ArgError> {
    let (slot, by_fd) = match name {
        "--bus-token-file" => (&mut raw.bus_token, false),
        "--bus-token-fd" => (&mut raw.bus_token, true),
        "--chain-key-file" => (&mut raw.chain_key, false),
        "--chain-key-fd" => (&mut raw.chain_key, true),
        _ => return Ok(false),
    };
    if slot.is_some() {
        return Err(ArgError::Arguments);
    }
    *slot = Some(if by_fd {
        let fd: i32 = number(value)?;
        if fd < FIRST_SECRET_FD {
            return Err(ArgError::Arguments);
        }
        SecretSource::Fd(fd)
    } else {
        SecretSource::File(PathBuf::from(value))
    });
    Ok(true)
}

fn limit_option(config: &mut CoreConfig, name: &str, value: &str) -> Result<(), ArgError> {
    match name {
        "--segment-bytes" => config.store.segment_bytes = number(value)?,
        "--total-bytes" => config.store.total_bytes = number(value)?,
        "--max-age-s" => config.store.max_age_ms = number::<u64>(value)?.saturating_mul(1000),
        "--ring-records" => config.ring_records = number(value)?,
        "--rate-burst" => {
            config.rate = RateLimit {
                burst: number(value)?,
                ..config.rate
            }
        }
        "--rate-per-second" => {
            config.rate = RateLimit {
                per_second: number(value)?,
                ..config.rate
            };
        }
        "--fsync" => config.store.fsync = fsync(value, config.store.fsync)?,
        _ => return Err(ArgError::Arguments),
    }
    Ok(())
}

fn finish(raw: Raw, config: CoreConfig) -> Result<DaemonCommand, ArgError> {
    if raw.help {
        return Ok(DaemonCommand::Help);
    }
    if raw.policy.is_some() && raw.allow_all {
        return Err(ArgError::PolicyConflict);
    }
    if raw.bus.is_some() && raw.policy.is_none() && !raw.allow_all {
        return Err(ArgError::NoPolicy);
    }
    if raw.listeners.iter().any(|l| l.name.is_none()) && raw.uid_map.is_none() {
        return Err(ArgError::NoUidMap);
    }
    if !config.store.is_valid() || config.ring_records == 0 {
        return Err(ArgError::Limits);
    }
    if raw.listeners.is_empty() {
        return Err(ArgError::Arguments);
    }
    Ok(DaemonCommand::Run(Box::new(DaemonArgs {
        dir: raw.dir.ok_or(ArgError::Arguments)?,
        listeners: raw.listeners,
        uid_map: raw.uid_map,
        bus: raw.bus,
        bus_token: raw.bus_token,
        policy: raw.policy,
        allow_all: raw.allow_all,
        chain_key: raw.chain_key,
        kernel_log: raw.kernel_log,
        config,
        allow_root: raw.allow_root,
    })))
}

/// Parses the words after the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<DaemonCommand, ArgError> {
    let mut args = args.into_iter();
    let mut raw = Raw::default();
    let mut config = CoreConfig::STANDARD;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => raw.help = true,
            "--allow-all" => raw.allow_all = true,
            "--allow-root" => raw.allow_root = true,
            name => {
                let value = args.next().ok_or(ArgError::Arguments)?;
                match name {
                    "--lang" => {}
                    "--listen" => raw.listeners.push(listen(&value)?),
                    "--mode" => {
                        let last = raw.listeners.last_mut().ok_or(ArgError::Arguments)?;
                        last.mode = Some(mode(&value)?);
                    }
                    _ if secret_option(&mut raw, name, &value)? => {}
                    _ if path_option(&mut raw, name, value.clone()) => {}
                    _ => limit_option(&mut config, name, &value)?,
                }
            }
        }
    }
    finish(raw, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<DaemonCommand, ArgError> {
        parse(words.iter().map(|w| (*w).to_owned()))
    }

    fn run(words: &[&str]) -> DaemonArgs {
        match parsed(words).unwrap() {
            DaemonCommand::Run(args) => *args,
            DaemonCommand::Help => panic!("expected a run command"),
        }
    }

    #[test]
    fn the_minimal_command_line_gets_the_standard_limits() {
        let minimal = run(&[
            "--dir",
            "/var/log/ultar",
            "--listen",
            "/run/l.sock=system.devd",
        ]);
        assert_eq!(minimal.dir, PathBuf::from("/var/log/ultar"));
        assert_eq!(minimal.listeners[0].name.as_deref(), Some("system.devd"));
        assert_eq!(minimal.config, CoreConfig::STANDARD);
        assert!(minimal.bus.is_none() && !minimal.allow_root);
    }

    /// Parses a command line given as one string of words.
    fn words(line: &str) -> DaemonArgs {
        let list: Vec<&str> = line.split_whitespace().collect();
        run(&list)
    }

    #[test]
    fn sockets_and_files_are_read() {
        let full = words(
            "--dir /d --listen /a.sock --mode 0666 --listen /b=c/k.sock=kernelish \
             --uid-map /m --bus /bus --policy /p --bus-token-file /t \
             --chain-key-file /k --kernel-log /proc/x --allow-root --lang en",
        );
        let first = ListenArg {
            path: "/a.sock".into(),
            name: None,
            mode: Some(0o666),
        };
        assert_eq!(full.listeners[0], first);
        assert_eq!(full.listeners[1].path, PathBuf::from("/b=c/k.sock"));
        assert_eq!(full.policy, Some(PathBuf::from("/p")));
        assert!(full.allow_root && !full.allow_all);
        assert_eq!(full.kernel_log, Some(PathBuf::from("/proc/x")));
        assert_eq!(full.chain_key, Some(SecretSource::File("/k".into())));
        assert_eq!(full.bus_token, Some(SecretSource::File("/t".into())));
        let by_fd = words("--dir /d --listen /l=s --chain-key-fd 3 --bus-token-fd 4");
        assert_eq!(by_fd.chain_key, Some(SecretSource::Fd(3)));
        assert_eq!(by_fd.bus_token, Some(SecretSource::Fd(4)));
    }

    #[test]
    fn limits_are_read() {
        let full = words(
            "--dir /d --listen /l=svc --segment-bytes 65536 --total-bytes 1048576 \
             --max-age-s 3600 --ring-records 100 --rate-burst 7 --rate-per-second 3 \
             --fsync always",
        );
        assert_eq!(full.config.store.segment_bytes, 65_536);
        assert_eq!(full.config.store.total_bytes, 1_048_576);
        assert_eq!(full.config.store.max_age_ms, 3_600_000);
        assert_eq!(full.config.store.fsync, FsyncPolicy::Always);
        assert_eq!(full.config.ring_records, 100);
        let rate = RateLimit {
            burst: 7,
            per_second: 3,
        };
        assert_eq!(full.config.rate, rate);
    }

    #[test]
    fn rejections() {
        let base = ["--dir", "/d", "--listen", "/l=svc"];
        let with = |extra: &[&str]| parsed(&[&base[..], extra].concat());
        assert_eq!(parsed(&["--help"]), Ok(DaemonCommand::Help));
        assert_eq!(parsed(&["--dir", "/d"]), Err(ArgError::Arguments));
        assert_eq!(parsed(&["--listen", "/l=svc"]), Err(ArgError::Arguments));
        assert_eq!(
            parsed(&["--dir", "/d", "--listen", "/l"]),
            Err(ArgError::NoUidMap)
        );
        assert_eq!(with(&["--bus", "/b"]), Err(ArgError::NoPolicy));
        assert_eq!(
            with(&["--bus", "/b", "--policy", "/p", "--allow-all"]),
            Err(ArgError::PolicyConflict)
        );
        assert_eq!(with(&["--segment-bytes", "10"]), Err(ArgError::Limits));
        assert_eq!(with(&["--total-bytes", "1048576"]), Err(ArgError::Limits));
        assert_eq!(with(&["--ring-records", "0"]), Err(ArgError::Limits));
        assert!(with(&["--bus", "/b", "--allow-all", "--fsync", "batch"]).is_ok());
        assert!(with(&["--fsync", "rotate"]).is_ok());
    }

    #[test]
    fn malformed_options_are_argument_errors() {
        let with =
            |extra: &[&str]| parsed(&[&["--dir", "/d", "--listen", "/l=svc"][..], extra].concat());
        for bad in [
            &["--frob", "1"][..],
            &["--dir"],
            &["--mode", "999"],
            &["--fsync", "sometimes"],
            &["--rate-burst", "-1"],
            &["--rate-burst", "many"],
            &["--listen", "=x"],
            &["--listen", "/p=Bad Name"],
            // A secret on the command line is not accepted.
            &["--token", "secret"],
            // Standard streams, and one secret from two sources.
            &["--chain-key-fd", "2"],
            &["--bus-token-fd", "x"],
            &["--chain-key-fd", "3", "--chain-key-file", "/k"],
            &["--bus-token-file", "/t", "--bus-token-fd", "4"],
            &["extra"],
        ] {
            assert_eq!(with(bad), Err(ArgError::Arguments), "{bad:?}");
        }
        // `--mode` needs a listener before it.
        assert_eq!(
            parsed(&["--mode", "600", "--dir", "/d", "--listen", "/l=s"]),
            Err(ArgError::Arguments)
        );
    }
}
