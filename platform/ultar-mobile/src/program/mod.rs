//! The two programs of the phone on the compositor and the bus:
//!
//! * [`shell`] – `ultar-mobile`: status bar and navigation bar as panels, the home screen as
//!   the background, quick settings, recent apps and search as a sheet between the bars;
//! * [`lock`] – `ultar-mobile-lock`: the lock screen (cover and sign-in) with the shared lock
//!   state (`ultar-lock-core`) and the authentication service.
//!
//! [`bus`] connects both to the system services. Without a bus the shell still runs: the
//! service parts show what they know (nothing) instead of failing.

pub mod bus;
pub mod bus_map;
pub mod lock;
pub mod lock_bus;
pub mod shell;
pub mod tz;

use crate::model::time::LocalTime;
use std::path::PathBuf;
use ultar_surface::proto::OutputInfo;
use ultar_ui::geom::Size;
use ultar_ui::Lang;

/// Command line of both programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The compositor's socket.
    pub compositor: PathBuf,
    /// The bus socket.
    pub bus: Option<PathBuf>,
    /// Language (Arabic when unset).
    pub lang: Lang,
    /// Light theme instead of the standard dark one.
    pub light: bool,
    /// The session's user (lock screen: only this person unlocks).
    pub user: Option<String>,
}

/// Errors of the programs.
#[derive(Debug)]
pub enum ProgramError {
    /// The compositor connection failed.
    Compositor(ultar_surface::client::ClientError),
    /// A tree could not be built.
    Ui(ultar_ui::ui::UiError),
}

impl From<ultar_surface::client::ClientError> for ProgramError {
    fn from(e: ultar_surface::client::ClientError) -> ProgramError {
        ProgramError::Compositor(e)
    }
}

impl From<ultar_ui::ui::UiError> for ProgramError {
    fn from(e: ultar_ui::ui::UiError) -> ProgramError {
        ProgramError::Ui(e)
    }
}

/// `--compositor PATH [--bus PATH] [--lang ar|en|de] [--light] [--user NAME]`; `None` on
/// anything else.
pub fn parse_options(args: impl Iterator<Item = String>) -> Option<Options> {
    let mut o = Options {
        compositor: PathBuf::from("/run/ultar/ui/compositor.sock"),
        bus: None,
        lang: Lang::Ar,
        light: false,
        user: None,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = PathBuf::from(args.next()?),
            "--bus" => o.bus = Some(PathBuf::from(args.next()?)),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            "--user" => o.user = Some(args.next()?),
            _ => return None,
        }
    }
    Some(o)
}

/// Now in milliseconds since the epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The local time in the zone `offset_s` seconds east of UTC.
pub fn local_now(offset_s: i32) -> Option<LocalTime> {
    LocalTime::at(now_ms(), offset_s)
}

/// Seconds east of UTC of zone `name` now.
pub fn zone_offset(name: &str) -> i32 {
    tz::offset(name, i64::try_from(now_ms() / 1000).unwrap_or(0))
}

/// The size of the first output in points (a phone has one; a phone-sized default before
/// the compositor said).
pub fn screen_size(outputs: &[OutputInfo]) -> Size {
    outputs.first().map_or(Size::new(360.0, 760.0), |o| {
        let s = o.scale_milli.max(1) as f32 / 1000.0;
        Size::new(o.width as f32 / s, o.height as f32 / s)
    })
}

/// Milliseconds until the next minute (when the clocks change).
pub fn ms_to_minute() -> u64 {
    60_000 - now_ms() % 60_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_parse() {
        let args = ["--bus", "/b", "--lang", "en", "--light", "--user", "anas"];
        let o = parse_options(args.iter().map(|s| (*s).to_owned())).unwrap();
        assert_eq!(o.bus, Some(PathBuf::from("/b")));
        assert_eq!(
            (o.lang, o.light, o.user.as_deref()),
            (Lang::En, true, Some("anas"))
        );
        assert!(parse_options(["--what"].iter().map(|s| (*s).to_owned())).is_none());
        assert!(parse_options(["--bus"].iter().map(|s| (*s).to_owned())).is_none());
    }

    #[test]
    fn a_phone_sized_screen_without_outputs() {
        assert_eq!(screen_size(&[]), Size::new(360.0, 760.0));
    }
}
