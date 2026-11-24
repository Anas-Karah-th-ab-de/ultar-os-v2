//! Command line of `ultar-busd`.

use std::path::PathBuf;
use ultar_bus::SocketAccess;
use ultar_service::SecretSource;

/// Identity of peers on a socket given without explicit identity.
pub const DEFAULT_IDENTITY: &str = "system";
/// Environment variable naming the default socket path (not a secret).
pub const SOCKET_ENV: &str = "ULTAR_BUS_SOCKET";

/// How the peers of one listener are identified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentitySource {
    /// Every peer is this identity; the socket file's owner and mode (or
    /// the mount that exposes it in a cell) decide who can connect.
    Fixed(String),
    /// Kernel credentials (`SO_PEERCRED`) looked up in the uid map.
    Credentials,
    /// A token presented in the handshake, looked up in the token file.
    Token,
    /// Apps: kernel credentials looked up in the launcher's app map
    /// (`--app-map`, `--app-map-owner`, `--app-uids`), see `apps.rs`.
    Apps,
}

/// One socket to listen on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// Path of the socket file.
    pub path: PathBuf,
    /// Who the peers are.
    pub source: IdentitySource,
    /// Mode, owner and group of the socket file.
    pub access: SocketAccess,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Arguments {
    /// Sockets to serve.
    pub listeners: Vec<Listener>,
    /// Policy file.
    pub policy: Option<PathBuf>,
    /// Run without policy (development).
    pub allow_all: bool,
    /// File that maps user ids to identities.
    pub uid_map: Option<PathBuf>,
    /// Table with one `identity = "token"` line per client: a checked
    /// file (`--token-file`) or an inherited descriptor (`--token-fd`).
    pub token: Option<SecretSource>,
    /// Ingestion socket of the log service.
    pub log_socket: Option<PathBuf>,
    /// The operator declares that the broker runs as root.
    pub allow_root: bool,
    /// Language of operator messages, when given.
    pub lang: Option<String>,
    /// Print the usage and exit.
    pub help: bool,
    /// The launcher's map of app user ids (`--app-map`).
    pub app_map: Option<PathBuf>,
    /// Who besides root may own the app map (`--app-map-owner`).
    pub app_map_owner: Option<u32>,
    /// The app user ids (`--app-uids FIRST-LAST`).
    pub app_uids: Option<(u32, u32)>,
}

/// Why the command line was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentError {
    /// An option needs a value that is missing.
    MissingValue,
    /// An option this program does not know (this includes `--token`:
    /// secrets are not accepted on the command line).
    UnknownOption,
    /// `--listen` without `PATH=IDENTITY`.
    BadListener,
    /// A number that is not one (`--mode`, `--owner`, `--group`,
    /// `--max-connections`, `--token-fd`), or out of range.
    BadNumber,
    /// `--mode`, `--owner` or `--group` before any listener.
    NoListenerYet,
    /// No socket was named.
    NoSocket,
    /// `--policy` together with `--allow-all`.
    PolicyConflict,
    /// A credential listener without `--uid-map`, or a token listener
    /// without `--token-file` or `--token-fd`.
    MissingSourceFile,
    /// `--token-file` together with `--token-fd`, or one of them twice.
    TwoTokenSources,
}

/// `--app-uids FIRST-LAST`: a non-empty range above the system ids.
fn parse_uid_range(text: &str) -> Result<(u32, u32), ArgumentError> {
    let (first, last) = text.split_once('-').ok_or(ArgumentError::BadNumber)?;
    let (first, last) = (parse_id(first)?, parse_id(last)?);
    if first < 1000 || first > last {
        return Err(ArgumentError::BadNumber);
    }
    Ok((first, last))
}

/// Most connections one listener may be given with `--max-connections`.
pub const MAX_CONNECTIONS_PER_LISTENER: usize = 1024;

fn fixed_listener(spec: &str) -> Result<Listener, ArgumentError> {
    match spec.rsplit_once('=') {
        Some((path, identity)) if !path.is_empty() && !identity.is_empty() => Ok(Listener {
            path: PathBuf::from(path),
            source: IdentitySource::Fixed(identity.to_owned()),
            access: SocketAccess::default(),
        }),
        _ => Err(ArgumentError::BadListener),
    }
}

fn plain_listener(path: String, source: IdentitySource) -> Result<Listener, ArgumentError> {
    if path.is_empty() {
        return Err(ArgumentError::BadListener);
    }
    Ok(Listener {
        path: PathBuf::from(path),
        source,
        access: SocketAccess::default(),
    })
}

/// Parses permission bits: one to four octal digits, at most `0o777`.
pub fn parse_mode(text: &str) -> Result<u32, ArgumentError> {
    let valid =
        !text.is_empty() && text.len() <= 4 && text.bytes().all(|b| (b'0'..=b'7').contains(&b));
    let mode = u32::from_str_radix(text, 8).map_err(|_| ArgumentError::BadNumber)?;
    if !valid || mode > 0o777 {
        return Err(ArgumentError::BadNumber);
    }
    Ok(mode)
}

fn parse_id(text: &str) -> Result<u32, ArgumentError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ArgumentError::BadNumber);
    }
    text.parse().map_err(|_| ArgumentError::BadNumber)
}

/// `--max-connections`: 1 to [`MAX_CONNECTIONS_PER_LISTENER`].
fn parse_connections(text: &str) -> Result<usize, ArgumentError> {
    let count = usize::try_from(parse_id(text)?).map_err(|_| ArgumentError::BadNumber)?;
    if count == 0 || count > MAX_CONNECTIONS_PER_LISTENER {
        return Err(ArgumentError::BadNumber);
    }
    Ok(count)
}

/// `--token-fd`: a descriptor above the standard streams.
fn parse_fd(text: &str) -> Result<i32, ArgumentError> {
    match i32::try_from(parse_id(text)?) {
        Ok(fd) if fd >= 3 => Ok(fd),
        _ => Err(ArgumentError::BadNumber),
    }
}

fn set_token(parsed: &mut Arguments, source: SecretSource) -> Result<(), ArgumentError> {
    if parsed.token.replace(source).is_some() {
        return Err(ArgumentError::TwoTokenSources);
    }
    Ok(())
}

fn last(parsed: &mut Arguments) -> Result<&mut SocketAccess, ArgumentError> {
    parsed
        .listeners
        .last_mut()
        .map(|listener| &mut listener.access)
        .ok_or(ArgumentError::NoListenerYet)
}

fn option(
    parsed: &mut Arguments,
    name: &str,
    args: &mut impl Iterator<Item = String>,
) -> Result<(), ArgumentError> {
    let mut value = || args.next().ok_or(ArgumentError::MissingValue);
    match name {
        "--policy" => parsed.policy = Some(PathBuf::from(value()?)),
        "--uid-map" => parsed.uid_map = Some(PathBuf::from(value()?)),
        "--token-file" => set_token(parsed, SecretSource::File(PathBuf::from(value()?)))?,
        "--token-fd" => set_token(parsed, SecretSource::Fd(parse_fd(&value()?)?))?,
        "--log-socket" => parsed.log_socket = Some(PathBuf::from(value()?)),
        "--lang" => parsed.lang = Some(value()?),
        "--listen" => parsed.listeners.push(fixed_listener(&value()?)?),
        "--listen-cred" => parsed
            .listeners
            .push(plain_listener(value()?, IdentitySource::Credentials)?),
        "--listen-token" => parsed
            .listeners
            .push(plain_listener(value()?, IdentitySource::Token)?),
        "--listen-apps" => parsed
            .listeners
            .push(plain_listener(value()?, IdentitySource::Apps)?),
        "--app-map" => parsed.app_map = Some(PathBuf::from(value()?)),
        "--app-map-owner" => parsed.app_map_owner = Some(parse_id(&value()?)?),
        "--app-uids" => parsed.app_uids = Some(parse_uid_range(&value()?)?),
        "--mode" => last(parsed)?.mode = Some(parse_mode(&value()?)?),
        "--owner" => last(parsed)?.owner = Some(parse_id(&value()?)?),
        "--group" => last(parsed)?.group = Some(parse_id(&value()?)?),
        "--max-connections" => last(parsed)?.connections = Some(parse_connections(&value()?)?),
        _ => return Err(ArgumentError::UnknownOption),
    }
    Ok(())
}

fn check(parsed: &Arguments) -> Result<(), ArgumentError> {
    if parsed.help {
        return Ok(());
    }
    if parsed.listeners.is_empty() {
        return Err(ArgumentError::NoSocket);
    }
    if parsed.policy.is_some() && parsed.allow_all {
        return Err(ArgumentError::PolicyConflict);
    }
    let needs = |source: &IdentitySource| parsed.listeners.iter().any(|l| l.source == *source);
    if needs(&IdentitySource::Credentials) && parsed.uid_map.is_none() {
        return Err(ArgumentError::MissingSourceFile);
    }
    if needs(&IdentitySource::Token) && parsed.token.is_none() {
        return Err(ArgumentError::MissingSourceFile);
    }
    let app_parts = (&parsed.app_map, parsed.app_map_owner, parsed.app_uids);
    if needs(&IdentitySource::Apps) && !matches!(app_parts, (Some(_), Some(_), Some(_))) {
        return Err(ArgumentError::MissingSourceFile);
    }
    Ok(())
}

/// Parses the words after the program name. `env_socket` is the content
/// of [`SOCKET_ENV`], used when no socket is named.
pub fn parse_arguments(
    mut args: impl Iterator<Item = String>,
    env_socket: Option<String>,
) -> Result<Arguments, ArgumentError> {
    let mut parsed = Arguments::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => parsed.help = true,
            "--allow-all" => parsed.allow_all = true,
            "--allow-root" => parsed.allow_root = true,
            name if name.starts_with('-') => option(&mut parsed, name, &mut args)?,
            path => parsed.listeners.push(plain_listener(
                path.to_owned(),
                IdentitySource::Fixed(DEFAULT_IDENTITY.to_owned()),
            )?),
        }
    }
    if parsed.listeners.is_empty() {
        if let Some(path) = env_socket.filter(|path| !path.is_empty()) {
            parsed.listeners.push(plain_listener(
                path,
                IdentitySource::Fixed(DEFAULT_IDENTITY.to_owned()),
            )?);
        }
    }
    check(&parsed)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: Option<&str>) -> Result<Arguments, ArgumentError> {
        parse_arguments(
            args.iter().map(|arg| (*arg).to_owned()),
            env.map(str::to_owned),
        )
    }

    fn fixed(identity: &str) -> IdentitySource {
        IdentitySource::Fixed(identity.to_owned())
    }

    #[test]
    fn positional_path_gets_the_default_identity() {
        let parsed = parse(&["/tmp/bus.sock", "--allow-all"], None).unwrap();
        assert_eq!(parsed.listeners.len(), 1);
        assert_eq!(parsed.listeners[0].path, PathBuf::from("/tmp/bus.sock"));
        assert_eq!(parsed.listeners[0].source, fixed("system"));
        assert!(parsed.allow_all && parsed.policy.is_none());
    }

    #[test]
    fn listen_and_policy_options_are_read() {
        let parsed = parse(
            &[
                "--listen",
                "/a/x.sock=ui",
                "--listen",
                "/b=c/y.sock=app.phone",
                "--policy",
                "p.toml",
            ],
            None,
        )
        .unwrap();
        assert_eq!(parsed.listeners[0].path, PathBuf::from("/a/x.sock"));
        assert_eq!(parsed.listeners[0].source, fixed("ui"));
        assert_eq!(parsed.listeners[1].path, PathBuf::from("/b=c/y.sock"));
        assert_eq!(parsed.listeners[1].source, fixed("app.phone"));
        assert_eq!(parsed.policy, Some(PathBuf::from("p.toml")));
    }

    #[test]
    fn environment_is_the_fallback_only() {
        let parsed = parse(&["--allow-all"], Some("/env.sock")).unwrap();
        assert_eq!(parsed.listeners[0].path, PathBuf::from("/env.sock"));
        let parsed = parse(&["/arg.sock", "--allow-all"], Some("/env.sock")).unwrap();
        assert_eq!(parsed.listeners.len(), 1);
        assert_eq!(parsed.listeners[0].path, PathBuf::from("/arg.sock"));
        assert_eq!(parse(&[], Some("")), Err(ArgumentError::NoSocket));
        assert_eq!(parse(&[], None), Err(ArgumentError::NoSocket));
        assert!(parse(&["--help"], None).unwrap().help);
    }

    #[test]
    fn access_options_apply_to_the_listener_before_them() {
        let parsed = parse(
            &[
                "--listen",
                "/s.sock=system",
                "--mode",
                "0600",
                "--owner",
                "0",
                "--listen-cred",
                "/a.sock",
                "--mode",
                "666",
                "--group",
                "1500",
                "--uid-map",
                "/m",
                "--policy",
                "/p",
            ],
            None,
        )
        .unwrap();
        let system = &parsed.listeners[0].access;
        assert_eq!(
            (system.mode, system.owner, system.group),
            (Some(0o600), Some(0), None)
        );
        let apps = &parsed.listeners[1];
        assert_eq!(apps.source, IdentitySource::Credentials);
        assert_eq!(
            (apps.access.mode, apps.access.owner, apps.access.group),
            (Some(0o666), None, Some(1500))
        );
        assert_eq!(parsed.uid_map, Some(PathBuf::from("/m")));
        for early in ["--mode", "--owner", "--group"] {
            assert_eq!(
                parse(&[early, "1", "/s", "--allow-all"], None),
                Err(ArgumentError::NoListenerYet)
            );
        }
    }

    #[test]
    fn modes_and_ids_are_parsed_strictly() {
        assert_eq!(parse_mode("600"), Ok(0o600));
        assert_eq!(parse_mode("0666"), Ok(0o666));
        assert_eq!(parse_mode("0"), Ok(0));
        for bad in ["", "888", "1777", "07777", "-1", "0o600", "rw", " 600"] {
            assert_eq!(parse_mode(bad), Err(ArgumentError::BadNumber), "{bad:?}");
        }
        for bad in ["", "-1", "+1", "root", "99999999999"] {
            assert_eq!(
                parse(&["/s", "--owner", bad, "--allow-all"], None),
                Err(ArgumentError::BadNumber),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn bad_command_lines_are_rejected() {
        assert_eq!(parse(&["--policy"], None), Err(ArgumentError::MissingValue));
        assert_eq!(
            parse(&["--listen", "nopath"], None),
            Err(ArgumentError::BadListener)
        );
        assert_eq!(
            parse(&["--listen", "=x"], None),
            Err(ArgumentError::BadListener)
        );
        assert_eq!(parse(&["--frob"], None), Err(ArgumentError::UnknownOption));
        // Secrets are not accepted on the command line.
        assert_eq!(
            parse(&["/s", "--token", "abc"], None),
            Err(ArgumentError::UnknownOption)
        );
        assert_eq!(
            parse(&["/s", "--policy", "/p", "--allow-all"], None),
            Err(ArgumentError::PolicyConflict)
        );
        assert_eq!(
            parse(&["--listen-cred", "/s", "--allow-all"], None),
            Err(ArgumentError::MissingSourceFile)
        );
        assert_eq!(
            parse(&["--listen-token", "/s", "--allow-all"], None),
            Err(ArgumentError::MissingSourceFile)
        );
        let token = parse(
            &["--listen-token", "/s", "--token-file", "/t", "--allow-all"],
            None,
        )
        .unwrap();
        assert_eq!(token.listeners[0].source, IdentitySource::Token);
        let rest = parse(
            &[
                "/s",
                "--allow-all",
                "--allow-root",
                "--log-socket",
                "/l",
                "--lang",
                "en",
            ],
            None,
        )
        .unwrap();
        assert!(rest.allow_root);
        assert_eq!(rest.log_socket, Some(PathBuf::from("/l")));
        assert_eq!(rest.lang.as_deref(), Some("en"));
    }

    #[test]
    fn connection_bounds_and_token_descriptors_are_read() {
        let parsed = parse(
            &[
                "--listen",
                "/a=x",
                "--max-connections",
                "8",
                "--listen-token",
                "/t",
                "--token-fd",
                "3",
                "--allow-all",
            ],
            None,
        )
        .unwrap();
        assert_eq!(parsed.listeners[0].access.connections, Some(8));
        assert_eq!(parsed.listeners[1].access.connections, None);
        assert_eq!(parsed.token, Some(SecretSource::Fd(3)));
        for bad in [
            &["--max-connections", "0"][..],
            &["--max-connections", "1025"],
            &["--token-fd", "2"],
        ] {
            let words = [&["/s", "--allow-all"][..], bad].concat();
            assert_eq!(
                parse(&words, None),
                Err(ArgumentError::BadNumber),
                "{bad:?}"
            );
        }
        assert_eq!(
            parse(
                &["/s", "--allow-all", "--token-fd", "3", "--token-file", "/t"],
                None
            ),
            Err(ArgumentError::TwoTokenSources)
        );
        assert_eq!(
            parse(&["--max-connections", "4", "/s", "--allow-all"], None),
            Err(ArgumentError::NoListenerYet)
        );
    }

    #[test]
    fn the_apps_listener_needs_map_owner_and_range() {
        let full = [
            "--listen-apps",
            "/run/ultar/appbus/bus.sock",
            "--mode",
            "0666",
            "--app-map",
            "/run/ultar/launcher/bus-apps.toml",
            "--app-map-owner",
            "805",
            "--app-uids",
            "20000-49999",
        ];
        let parsed = parse(&full, None).unwrap();
        assert_eq!(parsed.listeners[0].source, IdentitySource::Apps);
        assert_eq!(parsed.app_uids, Some((20000, 49999)));
        assert_eq!(parsed.app_map_owner, Some(805));
        for drop in [4, 6, 8] {
            let mut part: Vec<&str> = full.to_vec();
            part.drain(drop..drop + 2);
            assert_eq!(
                parse(&part, None).err(),
                Some(ArgumentError::MissingSourceFile)
            );
        }
        for bad in ["0-49999", "30000-20000", "20000", "a-b", "20000-"] {
            let mut part: Vec<&str> = full.to_vec();
            part[9] = bad;
            assert_eq!(
                parse(&part, None).err(),
                Some(ArgumentError::BadNumber),
                "{bad}"
            );
        }
    }
}
