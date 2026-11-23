//! The token file: the documented fallback identity source for kernels
//! without peer credentials.
//!
//! One line per client, `#` starts a comment:
//!
//! ```text
//! system.settingsd = "c8a1…"
//! app.phone        = "5f0e…"
//! ```
//!
//! The file is a secret: it is read with the owner and mode checks of
//! [`ultar_service::read_secret_file`] (no access for group or others), or
//! from a descriptor the service manager passes (`--token-fd`, for kernels
//! whose file owners and modes cannot be checked). Tokens shorter than
//! [`MIN_TOKEN_LEN`] bytes are refused – a guessable token is no identity.
//! The table is bounded: `ultar_service::MAX_SECRET_TABLE_LEN` bytes, [`MAX_TOKENS`]
//! entries (review round 3, R3-B3).

use std::collections::BTreeSet;
#[cfg(test)]
use std::path::Path;
use ultar_bus::{Identity, TokenIdentity};
#[cfg(test)]
use ultar_service::{read_secret_file_limited, SecretFilePolicy, MAX_SECRET_TABLE_LEN};
use ultar_service::{Secret, SecretError};

/// Shortest accepted token in bytes.
pub const MIN_TOKEN_LEN: usize = 16;
/// Most entries of a token table.
pub const MAX_TOKENS: usize = 1024;

/// Why a token file was refused. Carries no token text.
#[derive(Debug)]
pub enum TokenFileError {
    /// The file failed the secret-file checks.
    Secret(SecretError),
    /// The file is not UTF-8.
    NotText,
    /// A line is not `identity = "token"`.
    Syntax {
        /// 1-based line number.
        line: usize,
    },
    /// A token is shorter than [`MIN_TOKEN_LEN`].
    WeakToken {
        /// 1-based line number.
        line: usize,
    },
    /// The same token or the same identity appears twice.
    Duplicate {
        /// 1-based line number.
        line: usize,
    },
    /// More than [`MAX_TOKENS`] entries.
    TooMany,
}

impl std::fmt::Display for TokenFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Secret(error) => write!(f, "{error}"),
            Self::NotText => f.write_str("not text"),
            Self::Syntax { line } => write!(f, "line {line}: expected identity = \"token\""),
            Self::WeakToken { line } => write!(f, "line {line}: token too short"),
            Self::Duplicate { line } => write!(f, "line {line}: duplicate identity or token"),
            Self::TooMany => write!(f, "more than {MAX_TOKENS} entries"),
        }
    }
}

impl std::error::Error for TokenFileError {}

fn parse_line(line: &str) -> Option<Option<(Identity, &str)>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Some(None);
    }
    let (identity, token) = line.split_once('=')?;
    let identity = Identity::new(identity.trim()).ok()?;
    let token = token.trim().strip_prefix('"')?.strip_suffix('"')?;
    if token.contains('"') {
        return None;
    }
    Some(Some((identity, token)))
}

/// Parses the text of a token file.
pub fn parse(text: &str) -> Result<TokenIdentity, TokenFileError> {
    let mut auth = TokenIdentity::new();
    let mut identities: BTreeSet<String> = BTreeSet::new();
    let mut tokens: BTreeSet<&str> = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let Some(entry) = parse_line(line) else {
            return Err(TokenFileError::Syntax { line: line_number });
        };
        let Some((identity, token)) = entry else {
            continue;
        };
        if token.len() < MIN_TOKEN_LEN {
            return Err(TokenFileError::WeakToken { line: line_number });
        }
        if tokens.len() >= MAX_TOKENS {
            return Err(TokenFileError::TooMany);
        }
        // Not secret-dependent timing towards a peer: this runs once at
        // start over the operator's own file.
        if !identities.insert(identity.as_str().to_owned()) || !tokens.insert(token) {
            return Err(TokenFileError::Duplicate { line: line_number });
        }
        auth = auth.with_token(token, identity);
    }
    Ok(auth)
}

/// Parses a token table read from a file or a descriptor.
pub fn from_secret(secret: &Secret) -> Result<TokenIdentity, TokenFileError> {
    parse(secret.expose_str().ok_or(TokenFileError::NotText)?)
}

/// Reads and parses a token file (the daemon reads through the runtime,
/// which also takes descriptors).
#[cfg(test)]
pub fn load(path: &Path) -> Result<TokenIdentity, TokenFileError> {
    let secret = read_secret_file_limited(
        path,
        &SecretFilePolicy::for_current_user(),
        MAX_SECRET_TABLE_LEN,
    )
    .map_err(TokenFileError::Secret)?;
    from_secret(&secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use ultar_bus::{PeerIdentity, PeerInfo};

    const A: &str = "0123456789abcdef-a";
    const B: &str = "0123456789abcdef-b";

    fn identify(auth: &TokenIdentity, token: Option<&str>) -> Option<String> {
        let (stream, _other) = UnixStream::pair().unwrap();
        let peer = PeerInfo {
            stream: &stream,
            client_name: "t",
            token,
        };
        auth.identify(&peer).ok().map(|id| id.as_str().to_owned())
    }

    #[test]
    fn tokens_map_to_identities_and_unknown_tokens_are_refused() {
        let text = format!("# clients\nsystem.settingsd = \"{A}\"\n\n  app.phone=\"{B}\"  \n");
        let auth = parse(&text).unwrap();
        assert_eq!(
            identify(&auth, Some(A)).as_deref(),
            Some("system.settingsd")
        );
        assert_eq!(identify(&auth, Some(B)).as_deref(), Some("app.phone"));
        assert_eq!(identify(&auth, Some("0123456789abcdef-c")), None);
        assert_eq!(identify(&auth, None), None);
        assert_eq!(identify(&parse("").unwrap(), Some(A)), None);
    }

    #[test]
    fn faulty_files_are_refused_without_echoing_tokens() {
        let cases = [
            (format!("app.x = {A}"), "line 1: expected"),
            (format!("App X = \"{A}\""), "line 1: expected"),
            (format!("\napp.x \"{A}\""), "line 2: expected"),
            (format!("app.x = \"{A}\"extra\""), "line 1: expected"),
            ("app.x = \"short\"".to_owned(), "line 1: token too short"),
            (
                format!("app.x = \"{A}\"\napp.x = \"{B}\""),
                "line 2: duplicate",
            ),
            (
                format!("app.x = \"{A}\"\napp.y = \"{A}\""),
                "line 2: duplicate",
            ),
        ];
        for (text, expected) in cases {
            let message = parse(&text).unwrap_err().to_string();
            assert!(message.starts_with(expected), "{message}");
            assert!(!message.contains(A) && !message.contains("short\""));
        }
    }

    #[test]
    fn a_table_is_bounded_in_entries() {
        let line = |n: usize| format!("app.c{n} = \"0123456789abcdef-{n:05}\"\n");
        let full: String = (0..MAX_TOKENS).map(line).collect();
        assert!(parse(&full).is_ok());
        let over = format!("{full}{}", line(MAX_TOKENS));
        assert!(matches!(parse(&over), Err(TokenFileError::TooMany)));
    }

    #[test]
    fn the_file_must_be_private() {
        let dir = std::env::temp_dir().join(format!("ultar-busd-tok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tokens");
        std::fs::write(&path, format!("app.x = \"{A}\"\n")).unwrap();
        let chmod = |mode| {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        chmod(0o600);
        assert!(load(&path).is_ok());
        chmod(0o644);
        assert!(matches!(
            load(&path),
            Err(TokenFileError::Secret(SecretError::TooPermissive))
        ));
        chmod(0o600);
        std::fs::write(&path, [0xff, 0xfe, b'x']).unwrap();
        assert!(matches!(load(&path), Err(TokenFileError::NotText)));
        assert!(matches!(
            load(&dir.join("missing")),
            Err(TokenFileError::Secret(SecretError::Io(_)))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
