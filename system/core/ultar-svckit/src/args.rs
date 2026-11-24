//! The command line every small service shares.
//!
//! ```text
//! <service> --bus PATH [--log-socket PATH] [--token-file PATH] [--allow-root] [--<option> VALUE …]
//! ```
//! The bus socket may also come from `ULTAR_BUS_SOCKET`. Paths must be absolute; a
//! service names its own options (each takes exactly one value) and checks their values.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Environment variable with the bus socket.
pub const SOCKET_ENV: &str = "ULTAR_BUS_SOCKET";
/// Longest accepted option value in bytes.
pub const MAX_VALUE: usize = 4096;

/// The shared part of the command line plus the service's own options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonArgs {
    /// Bus socket.
    pub bus: PathBuf,
    /// Socket of the log service.
    pub log_socket: Option<PathBuf>,
    /// File with the bus token.
    pub token_file: Option<PathBuf>,
    /// Start even as root (development only).
    pub allow_root: bool,
    /// The service's own options (`--name value`), by name without dashes.
    pub options: BTreeMap<String, String>,
}

/// The command line was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageError;

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid options")
    }
}

impl std::error::Error for UsageError {}

impl CommonArgs {
    /// An own option as text.
    pub fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(String::as_str)
    }

    /// An own option that must be an absolute path.
    pub fn path(&self, name: &str) -> Result<Option<PathBuf>, UsageError> {
        match self.option(name) {
            None => Ok(None),
            Some(v) if v.starts_with('/') => Ok(Some(PathBuf::from(v))),
            Some(_) => Err(UsageError),
        }
    }

    /// An own option that must be present and an absolute path.
    pub fn required_path(&self, name: &str) -> Result<PathBuf, UsageError> {
        self.path(name)?.ok_or(UsageError)
    }
}

/// Reads the command line (`Ok(None)`: help was asked for). `known` lists the
/// service's own options (without dashes); every other word is refused.
pub fn parse(
    words: impl IntoIterator<Item = String>,
    env_socket: Option<String>,
    known: &[&str],
) -> Result<Option<CommonArgs>, UsageError> {
    let mut bus = env_socket.filter(|s| !s.is_empty()).map(PathBuf::from);
    let mut log_socket = None;
    let mut token_file = None;
    let mut allow_root = false;
    let mut options = BTreeMap::new();
    let mut it = words.into_iter();
    while let Some(word) = it.next() {
        let mut value = || {
            it.next()
                .filter(|v| !v.is_empty() && v.len() <= MAX_VALUE)
                .ok_or(UsageError)
        };
        match word.as_str() {
            "--help" | "-h" => return Ok(None),
            "--bus" => bus = Some(PathBuf::from(value()?)),
            "--log-socket" => log_socket = Some(PathBuf::from(value()?)),
            "--token-file" => token_file = Some(PathBuf::from(value()?)),
            "--allow-root" => allow_root = true,
            other => {
                let name = other.strip_prefix("--").ok_or(UsageError)?;
                if !known.contains(&name) || options.contains_key(name) {
                    return Err(UsageError);
                }
                options.insert(name.to_owned(), value()?);
            }
        }
    }
    let bus = bus.ok_or(UsageError)?;
    let absolute = std::iter::once(&bus)
        .chain(&log_socket)
        .chain(&token_file)
        .all(|p| p.is_absolute());
    if !absolute {
        return Err(UsageError);
    }
    Ok(Some(CommonArgs {
        bus,
        log_socket,
        token_file,
        allow_root,
        options,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn shared_and_own_options_are_read() {
        let a = parse(
            words("--bus /run/b.sock --log-socket /run/l.sock --dir /data/x --allow-root"),
            None,
            &["dir"],
        )
        .unwrap()
        .unwrap();
        assert_eq!(a.bus, PathBuf::from("/run/b.sock"));
        assert!(a.allow_root);
        assert_eq!(a.required_path("dir").unwrap(), PathBuf::from("/data/x"));
        assert_eq!(a.path("other").unwrap(), None);
        assert_eq!(a.option("dir"), Some("/data/x"));
    }

    #[test]
    fn environment_gives_the_bus_and_help_is_none() {
        let a = parse(Vec::new(), Some("/run/e.sock".into()), &[])
            .unwrap()
            .unwrap();
        assert_eq!(a.bus, PathBuf::from("/run/e.sock"));
        assert_eq!(parse(words("--help"), None, &[]).unwrap(), None);
    }

    #[test]
    fn bad_command_lines_are_refused() {
        for line in [
            "",
            "--bus",
            "--bus rel.sock",
            "--bus /b --unknown 1",
            "--bus /b --dir /a --dir /b",
            "--bus /b --log-socket rel",
            "--bus /b stray",
        ] {
            assert_eq!(
                parse(words(line), None, &["dir"]),
                Err(UsageError),
                "{line}"
            );
        }
        let a = parse(words("--bus /b --dir rel"), None, &["dir"])
            .unwrap()
            .unwrap();
        assert_eq!(a.required_path("dir"), Err(UsageError));
        assert_eq!(a.required_path("missing"), Err(UsageError));
        let long = format!("--bus /{}", "x".repeat(MAX_VALUE + 1));
        assert_eq!(parse(words(&long), None, &[]), Err(UsageError));
        assert_eq!(UsageError.to_string(), "invalid options");
    }
}
