//! The three programs' main loops (the binaries only parse their command line): the
//! desktop shell, the lock screen and the system dialogs.

pub mod desktop;
pub mod keyboard;
pub mod lock;
pub mod sysdialog;

use crate::model::BarPlace;
use std::fmt;
use std::path::PathBuf;
use ultar_i18n::Lang;
use ultar_surface::client::ClientError;
use ultar_surface::proto::{OutputId, OutputInfo};
use ultar_ui::ui::UiError;

/// Where to connect and how to look.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// The compositor's socket.
    pub compositor: PathBuf,
    /// The bus socket (no bus: the services' parts are disabled with their reason).
    pub bus: Option<PathBuf>,
    /// Language (from the session; Arabic when unset).
    pub lang: Lang,
    /// Light theme instead of the standard dark one (until `appearance.theme` is read).
    pub light: bool,
    /// Where the desktop's bar is (`--bar top|bottom`); without it the setting
    /// `bar.layout` decides, and the bottom is the standard.
    pub bar: Option<BarPlace>,
}

/// Why a program stopped.
#[derive(Debug)]
pub enum AppError {
    /// The compositor connection failed.
    Compositor(ClientError),
    /// A tree could not be built.
    Ui(UiError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Compositor(e) => write!(f, "compositor: {e}"),
            AppError::Ui(e) => write!(f, "ui: {e:?}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<ClientError> for AppError {
    fn from(e: ClientError) -> AppError {
        AppError::Compositor(e)
    }
}

impl From<UiError> for AppError {
    fn from(e: UiError) -> AppError {
        AppError::Ui(e)
    }
}

/// Parses the common options (`--compositor PATH --bus PATH --lang ar|en|de --light`);
/// returns the words it did not know.
pub fn parse_options(args: impl Iterator<Item = String>) -> Option<(Options, Vec<String>)> {
    let mut o = Options {
        compositor: PathBuf::from("/run/ultar/ui/compositor.sock"),
        bus: None,
        lang: Lang::from_env(),
        light: false,
        bar: None,
    };
    let mut rest = Vec::new();
    let mut args = args.peekable();
    while let Some(word) = args.next() {
        match word.as_str() {
            "--compositor" => o.compositor = PathBuf::from(args.next()?),
            "--bus" => o.bus = Some(PathBuf::from(args.next()?)),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            "--bar" => o.bar = Some(BarPlace::parse(&args.next()?)?),
            _ => rest.push(word),
        }
    }
    Some((o, rest))
}

/// The primary output (or the first).
pub fn primary(outputs: &[OutputInfo]) -> Option<&OutputInfo> {
    outputs
        .iter()
        .find(|o| o.primary)
        .or_else(|| outputs.first())
}

/// A built-in panel (laptop, phone): by connector name.
pub fn internal(o: &OutputInfo) -> bool {
    let n = o.name.to_ascii_lowercase();
    ["edp", "lvds", "dsi"].iter().any(|p| n.starts_with(p))
}

/// The outputs Super+P switches between: this device's (the built-in one, else the
/// primary) and the first other one (§13 no. 44).
pub fn mode_outputs(outputs: &[OutputInfo]) -> (Option<OutputId>, Option<OutputId>) {
    let first = outputs
        .iter()
        .find(|o| internal(o))
        .or_else(|| primary(outputs))
        .map(|o| o.id);
    let second = outputs.iter().map(|o| o.id).find(|id| Some(*id) != first);
    (first, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(id: u32, name: &str, primary: bool) -> OutputInfo {
        OutputInfo {
            id: OutputId(id),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_milli: 1000,
            refresh_millihertz: 60_000,
            primary,
            name: name.into(),
        }
    }

    #[test]
    fn options_and_outputs() {
        let args = [
            "--lang", "en", "--bus", "/b", "--x", "--light", "--bar", "top",
        ]
        .map(String::from);
        let (o, rest) = parse_options(args.into_iter()).unwrap();
        assert_eq!(
            (o.lang, o.bus.as_deref(), o.light),
            (Lang::En, Some(std::path::Path::new("/b")), true)
        );
        assert_eq!(rest, vec!["--x".to_owned()]);
        assert_eq!(o.bar, Some(BarPlace::Top));
        assert!(parse_options(["--bar".to_owned(), "left".to_owned()].into_iter()).is_none());
        assert!(parse_options(["--lang".to_owned()].into_iter()).is_none());
        let outs = [out(1, "HDMI-A-1", true), out(2, "eDP-1", false)];
        assert_eq!(mode_outputs(&outs), (Some(OutputId(2)), Some(OutputId(1))));
        assert_eq!(mode_outputs(&outs[..1]), (Some(OutputId(1)), None));
        assert_eq!(primary(&outs).unwrap().id, OutputId(1));
        assert!(!AppError::Ui(UiError::InvalidSize).to_string().is_empty());
    }
}
