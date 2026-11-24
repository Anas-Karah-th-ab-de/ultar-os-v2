//! What a start may ask an app to open (the desktop shell's request): one
//! URI of a scheme the app's manifest declares. The launcher checks it
//! strictly and hands it to the app as data (`ULTAR_OPEN_URI`); it never
//! interprets it.

use crate::error::LaunchError;
use crate::instances::Key;
use crate::launcher::{app_id, Core, Launcher};
use crate::manifest::ManifestError;
use std::fmt;
use std::sync::Arc;
use ultar_bus::messages::launcher::MAX_URI;
use ultar_bus::messages::launcher::{Role, StartReply};

/// Environment variable with the URI for a fresh instance (not the command
/// line: other users can read command lines).
pub const OPEN_ENV: &str = "ULTAR_OPEN_URI";
/// Most schemes one manifest declares.
pub const MAX_SCHEMES: usize = 16;
/// Longest scheme name.
const MAX_SCHEME: usize = 32;

/// What the caller of a start asks for besides the app.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Opening {
    /// The compositor's activation token for the window.
    pub token: Option<u64>,
    /// A URI to open (checked with [`check`]).
    pub uri: Option<String>,
    /// Another window even when the app runs.
    pub new_window: bool,
}

/// Why a URI is refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UriError {
    /// Empty or longer than `MAX_URI` bytes.
    Length,
    /// A control character (also line feeds, tabs, NUL, DEL).
    Control,
    /// No `scheme:` at its start, or a malformed scheme.
    Scheme,
    /// A scheme the app does not declare.
    Undeclared,
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "launcher.uri.length",
            Self::Control => "launcher.uri.control",
            Self::Scheme => "launcher.uri.scheme",
            Self::Undeclared => "launcher.uri.undeclared",
        })
    }
}

impl std::error::Error for UriError {}

/// Whether `name` is a scheme name: a lower-case letter, then lower-case
/// letters, digits, `+ - .`, at most 32 bytes (RFC 3986, lower case only).
pub fn scheme_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.len() <= MAX_SCHEME
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+-.".contains(b))
}

/// Whether `arg` may be an `[open] page_arg`: an option word (`--` and 1–30 lower-case
/// letters, digits or `-`).
pub fn page_arg_ok(arg: &str) -> bool {
    let Some(word) = arg.strip_prefix("--") else {
        return false;
    };
    (1..=30).contains(&word.len())
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !word.starts_with('-')
}

/// The page a URI names for an app with an `[open] page_arg`: the first part after
/// `<scheme>://` (`settings://network/vpn` → `network`), 1–32 lower-case letters, digits,
/// `_` or `-`. Anything else names no page (the app then opens at its start).
pub fn page(uri: &str) -> Option<&str> {
    let (_, rest) = uri.split_once("://")?;
    let page = rest.split(['/', '?', '#']).next()?;
    let ok = (1..=32).contains(&page.len())
        && page
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        && !page.starts_with('-');
    ok.then_some(page)
}

/// The `[open] schemes` of a manifest, checked.
pub fn schemes(names: Vec<String>) -> Result<Vec<String>, ManifestError> {
    if names.len() > MAX_SCHEMES || !names.iter().all(|n| scheme_ok(n)) {
        return Err(ManifestError::Permission);
    }
    Ok(names)
}

/// Checks `uri` against the app's declared `schemes`.
pub fn check(uri: &str, schemes: &[String]) -> Result<(), UriError> {
    if uri.is_empty() || uri.len() > MAX_URI {
        return Err(UriError::Length);
    }
    if uri.chars().any(char::is_control) {
        return Err(UriError::Control);
    }
    let (scheme, _) = uri.split_once(':').ok_or(UriError::Scheme)?;
    if !scheme_ok(scheme) {
        return Err(UriError::Scheme);
    }
    if !schemes.iter().any(|s| s == scheme) {
        return Err(UriError::Undeclared);
    }
    Ok(())
}

impl Core {
    /// Checks a URI against the app's manifest (and that only a window is
    /// asked to open one). Returns whether the app opens more windows.
    pub(crate) fn check_opening(&self, key: &Key, opening: &Opening) -> Result<bool, LaunchError> {
        if opening.uri.is_none() && !opening.new_window {
            return Ok(false);
        }
        self.installed(&key.0)?;
        let manifest = self.manifest(&key.0)?;
        if let Some(uri) = &opening.uri {
            if key.1 != Role::Ui {
                return Err(LaunchError::Invalid);
            }
            check(uri, &manifest.open_schemes).map_err(|_| LaunchError::Invalid)?;
        }
        Ok(manifest.multi_window)
    }
}

impl Launcher {
    /// Starts `role` of `app`; a running instance is reported, not started
    /// twice. `token`: the compositor's activation token for the window.
    pub fn start(
        self: &Arc<Self>,
        app: &str,
        role: Role,
        token: Option<u64>,
    ) -> Result<StartReply, LaunchError> {
        let opening = Opening {
            token,
            ..Opening::default()
        };
        self.start_opening(app, role, &opening)
    }

    /// Starts `role` of `app` to open `opening` (a URI, another window).
    pub fn start_opening(
        self: &Arc<Self>,
        app: &str,
        role: Role,
        opening: &Opening,
    ) -> Result<StartReply, LaunchError> {
        let key = (app_id(app)?, role);
        self.start_key(key, opening, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_of_uris_and_page_arguments() {
        assert_eq!(page("settings://network"), Some("network"));
        assert_eq!(page("settings://network/vpn"), Some("network"));
        assert_eq!(page("settings://apps/org.x?y"), Some("apps"));
        assert_eq!(page("settings://"), None);
        assert_eq!(page("settings://--help"), None);
        assert_eq!(page("settings://Net"), None);
        assert_eq!(page("settings:network"), None);
        assert_eq!(page("settings://a b"), None);
        assert!(page_arg_ok("--area"));
        assert!(!page_arg_ok("area") && !page_arg_ok("---x") && !page_arg_ok("--"));
        assert!(!page_arg_ok("--a=b"));
    }

    fn declared() -> Vec<String> {
        vec!["trash".into(), "settings".into(), "file".into()]
    }

    #[test]
    fn declared_uris_pass() {
        for uri in [
            "trash:///",
            "settings://network",
            "file:///data/a%20b.txt",
            "settings:",
        ] {
            assert_eq!(check(uri, &declared()), Ok(()), "{uri}");
        }
    }

    #[test]
    fn everything_else_is_refused() {
        let long = format!("trash:///{}", "a".repeat(MAX_URI));
        let cases: [(&str, UriError); 12] = [
            ("", UriError::Length),
            (&long, UriError::Length),
            ("trash:///\n", UriError::Control),
            ("trash:///\u{0}", UriError::Control),
            ("trash:///\u{7f}", UriError::Control),
            ("trash:///\u{9b}", UriError::Control),
            ("trash:///\t", UriError::Control),
            ("no-scheme", UriError::Scheme),
            ("Trash:///", UriError::Scheme),
            ("1x:///", UriError::Scheme),
            (":///", UriError::Scheme),
            ("store://app/x", UriError::Undeclared),
        ];
        for (uri, error) in cases {
            assert_eq!(check(uri, &declared()), Err(error), "{uri:?}");
            assert!(error.to_string().starts_with("launcher.uri."));
        }
        assert_eq!(check("trash:///", &[]), Err(UriError::Undeclared));
    }

    #[test]
    fn scheme_lists_are_bounded_and_checked() {
        assert!(schemes(vec!["a+b.c-d".into()]).is_ok());
        assert_eq!(schemes(vec!["A".into()]), Err(ManifestError::Permission));
        assert_eq!(
            schemes(vec!["x".repeat(33)]),
            Err(ManifestError::Permission)
        );
        let many = (0..=MAX_SCHEMES).map(|n| format!("s{n}")).collect();
        assert_eq!(schemes(many), Err(ManifestError::Permission));
    }
}
