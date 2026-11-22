//! Edition: the build and service preset of an image.
//!
//! The edition decides which services an image starts by default. It must not be used
//! to decide behaviour at run time; that is what [`crate::Capabilities`] are for.

use crate::msg::{self, Lang};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Kernel command line parameter that forces the edition.
pub const EDITION_CMDLINE_KEY: &str = "ultar.edition=";

/// Build and service preset of an Ultar OS image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edition {
    /// Phones and tablets.
    Phone,
    /// Laptops and desktop computers.
    Desktop,
    /// Servers, mini PCs used as servers, virtual machines without a screen.
    Server,
}

impl Edition {
    /// Every edition.
    pub const ALL: [Edition; 3] = [Edition::Phone, Edition::Desktop, Edition::Server];

    /// Parses an edition name. Accepts the aliases the old system accepted.
    pub fn parse(name: &str) -> Option<Edition> {
        match name.trim().to_ascii_lowercase().as_str() {
            "phone" | "handy" | "mobile" | "tablet" => Some(Edition::Phone),
            "desktop" | "laptop" | "pc" => Some(Edition::Desktop),
            "server" => Some(Edition::Server),
            _ => None,
        }
    }

    /// Stable name as written into images and service files.
    pub const fn as_str(self) -> &'static str {
        match self {
            Edition::Phone => "phone",
            Edition::Desktop => "desktop",
            Edition::Server => "server",
        }
    }

    /// Stable message key of the display name.
    pub const fn key(self) -> &'static str {
        match self {
            Edition::Phone => "profile.edition.phone",
            Edition::Desktop => "profile.edition.desktop",
            Edition::Server => "profile.edition.server",
        }
    }

    /// Display name in the given language.
    pub fn label(self, lang: Lang) -> &'static str {
        msg::text(self.key(), lang).unwrap_or(self.as_str())
    }

    /// True if this preset ships an on-screen interface.
    pub fn has_ui(self) -> bool {
        self != Edition::Server
    }

    /// Checks an `editions = [...]` entry of a service: an empty list or `all` matches every edition.
    pub fn matches<S: AsRef<str>>(self, list: &[S]) -> bool {
        list.is_empty()
            || list.iter().any(|entry| {
                let entry = entry.as_ref();
                entry.trim().eq_ignore_ascii_case("all") || Edition::parse(entry) == Some(self)
            })
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resolves the edition from its sources, in this order:
/// kernel command line `ultar.edition=` (last valid one wins), environment value,
/// edition file (first line that is neither empty nor a `#` comment), detected edition,
/// and finally [`Edition::Phone`].
pub fn resolve_edition(
    cmdline: &str,
    env: Option<&str>,
    file: Option<&str>,
    detected: Option<Edition>,
) -> Edition {
    let from_cmdline = cmdline
        .split_whitespace()
        .filter_map(|argument| argument.strip_prefix(EDITION_CMDLINE_KEY))
        .filter_map(Edition::parse)
        .next_back();
    if let Some(edition) = from_cmdline {
        return edition;
    }
    if let Some(edition) = env.and_then(Edition::parse) {
        return edition;
    }
    let from_file = file.and_then(|text| {
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .and_then(Edition::parse)
    });
    from_file.or(detected).unwrap_or(Edition::Phone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_names() {
        for edition in Edition::ALL {
            assert_eq!(Edition::parse(edition.as_str()), Some(edition));
            assert_eq!(edition.to_string(), edition.as_str());
        }
        assert_eq!(Edition::parse(" Laptop\n"), Some(Edition::Desktop));
        assert_eq!(Edition::parse("handy"), Some(Edition::Phone));
        assert_eq!(Edition::parse("tablet"), Some(Edition::Phone));
        assert_eq!(Edition::parse("pc"), Some(Edition::Desktop));
        assert_eq!(Edition::parse("toaster"), None);
        assert_eq!(Edition::parse(""), None);
    }

    // The following three tests carry over the tested behaviour of the old `ultar-edition`.
    #[test]
    fn source_order() {
        assert_eq!(resolve_edition("", None, None, None), Edition::Phone);
        assert_eq!(
            resolve_edition("", None, Some("# x\nserver\n"), None),
            Edition::Server
        );
        assert_eq!(
            resolve_edition("", Some("desktop"), Some("server"), None),
            Edition::Desktop
        );
        assert_eq!(
            resolve_edition(
                "quiet ultar.edition=laptop",
                Some("server"),
                Some("phone"),
                None
            ),
            Edition::Desktop
        );
        assert_eq!(
            resolve_edition("ultar.edition=bogus", None, Some("server"), None),
            Edition::Server
        );
    }

    #[test]
    fn setting_beats_detection() {
        assert_eq!(
            resolve_edition("", None, Some("phone"), Some(Edition::Desktop)),
            Edition::Phone
        );
        assert_eq!(
            resolve_edition("", None, None, Some(Edition::Desktop)),
            Edition::Desktop
        );
    }

    #[test]
    fn service_list_matching() {
        let none: [&str; 0] = [];
        assert!(Edition::Server.matches(&none));
        assert!(Edition::Server.matches(&["desktop".to_string(), "server".to_string()]));
        assert!(!Edition::Phone.matches(&["desktop", "server"]));
        assert!(Edition::Phone.matches(&["all"]));
    }

    #[test]
    fn cmdline_edge_cases() {
        // The last valid parameter wins; invalid ones are skipped.
        assert_eq!(
            resolve_edition(
                "ultar.edition=server ultar.edition=phone ultar.edition=x",
                None,
                None,
                None
            ),
            Edition::Phone
        );
        // A different parameter with the same suffix is not taken.
        assert_eq!(
            resolve_edition("foo.ultar.edition=server", None, None, None),
            Edition::Phone
        );
        // An invalid environment value falls through to the file.
        assert_eq!(
            resolve_edition("", Some("nonsense"), Some("\n\n  desktop  \n"), None),
            Edition::Desktop
        );
        // An invalid first file line falls through to detection.
        assert_eq!(
            resolve_edition("", None, Some("garbage\nserver"), Some(Edition::Server)),
            Edition::Server
        );
    }

    #[test]
    fn labels_and_ui() {
        assert!(Edition::Phone.has_ui() && Edition::Desktop.has_ui());
        assert!(!Edition::Server.has_ui());
        assert_eq!(Edition::Server.label(Lang::Ar), "إصدار الخادم");
        assert_eq!(Edition::Phone.label(Lang::En), "Phone edition");
        assert_eq!(Edition::Desktop.label(Lang::De), "Desktop-/Laptop-Ausgabe");
    }
}
