//! The command line: `ultar-wayland [options] -- program [args…]`.

use std::ffi::OsString;
use std::path::PathBuf;

/// Default socket of the compositor (overridden by `ULTAR_COMPOSITOR_SOCKET`).
pub const DEFAULT_COMPOSITOR: &str = "/run/ultar/ui/compositor.sock";
/// Name of the Wayland socket in the runtime directory.
pub const SOCKET_NAME: &str = "wayland-0";

/// What to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// The compositor socket.
    pub compositor: PathBuf,
    /// Directory of the Wayland socket (`XDG_RUNTIME_DIR` of the app).
    pub runtime_dir: Option<PathBuf>,
    /// App id told to the compositor (a hint; identity comes from the uid).
    pub app_id: String,
    /// The program and its arguments.
    pub program: Vec<OsString>,
}

/// Parses the arguments after the program name; `None` if they make no sense.
pub fn parse(
    args: Vec<OsString>,
    env_compositor: Option<OsString>,
    env_app: Option<String>,
) -> Option<Args> {
    let mut it = args.into_iter();
    let mut out = Args {
        compositor: PathBuf::from(env_compositor.unwrap_or_else(|| DEFAULT_COMPOSITOR.into())),
        runtime_dir: None,
        app_id: env_app.unwrap_or_else(|| "foreign".to_owned()),
        program: Vec::new(),
    };
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--") => {
                out.program = it.collect();
                break;
            }
            Some("--compositor") => out.compositor = PathBuf::from(it.next()?),
            Some("--runtime-dir") => out.runtime_dir = Some(PathBuf::from(it.next()?)),
            Some("--app-id") => out.app_id = it.next()?.into_string().ok()?,
            _ => return None,
        }
    }
    let valid_id = !out.app_id.is_empty()
        && out.app_id.len() <= ultar_surface::proto::MAX_APP_ID
        && out
            .app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    (!out.program.is_empty() && valid_id).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_program_and_options() {
        let a = parse(
            os(&["--app-id", "km.editor", "--", "gedit", "-s"]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(a.program, os(&["gedit", "-s"]));
        assert_eq!(a.compositor, PathBuf::from(DEFAULT_COMPOSITOR));
        assert_eq!(a.app_id, "km.editor");
        let b = parse(
            os(&["--compositor", "/s", "--runtime-dir", "/r", "--", "x"]),
            None,
            Some("a".into()),
        )
        .unwrap();
        assert_eq!(
            (b.compositor, b.runtime_dir, b.app_id),
            (PathBuf::from("/s"), Some(PathBuf::from("/r")), "a".into())
        );
        let c = parse(os(&["--", "x"]), Some("/env".into()), None).unwrap();
        assert_eq!(c.compositor, PathBuf::from("/env"));
    }

    #[test]
    fn refuses_nonsense() {
        assert!(parse(os(&[]), None, None).is_none());
        assert!(parse(os(&["--"]), None, None).is_none());
        assert!(parse(os(&["--bogus", "--", "x"]), None, None).is_none());
        assert!(parse(os(&["--compositor"]), None, None).is_none());
        assert!(parse(os(&["--app-id", "a b", "--", "x"]), None, None).is_none());
        assert!(parse(os(&["--app-id", &"a".repeat(65), "--", "x"]), None, None).is_none());
    }
}
