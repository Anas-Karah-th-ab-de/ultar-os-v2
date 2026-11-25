//! The Store's deep link: `store://app/<id>` opens an app's page, `store://app/<id>?action=remove`
//! opens it with the removal dialog (the shell's "Details in Store" / "Remove app",
//! docs/integration/desktop-shell.md). The launcher hands the URI to a fresh instance as data in
//! `ULTAR_OPEN_URI` (never on the command line); the manifest declares the scheme
//! (`[open] schemes = ["store"]`).
//!
//! The id is checked with the catalogue's own rule ([`ultar_catalog::model::valid_catalog_id`])
//! before it reaches the state: a malformed or unsafe id opens nothing (the Store starts normally).

use crate::action::StoreAction;
use ultar_catalog::model::valid_catalog_id;

/// What a `store://` URI asks the Store to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    /// The app's page (`store://app/<id>`).
    App(String),
    /// The app's page with the removal dialog (`store://app/<id>?action=remove`).
    Remove(String),
}

/// The environment variable the launcher sets for a fresh instance.
pub const OPEN_ENV: &str = "ULTAR_OPEN_URI";

/// Parses a `store://app/<id>[?action=remove]` URI; anything else is `None`.
pub fn parse(uri: &str) -> Option<DeepLink> {
    let rest = uri.strip_prefix("store://app/")?;
    let (id, query) = match rest.split_once('?') {
        Some((id, query)) => (id, query),
        None => (rest, ""),
    };
    if !valid_catalog_id(id) {
        return None;
    }
    Some(if query == "action=remove" {
        DeepLink::Remove(id.to_string())
    } else {
        DeepLink::App(id.to_string())
    })
}

/// The deep link the launcher handed this instance, if any.
pub fn from_env() -> Option<DeepLink> {
    std::env::var(OPEN_ENV).ok().and_then(|uri| parse(&uri))
}

impl DeepLink {
    /// The actions that open what the URI names (the removal asks first, as a click does).
    pub fn actions(&self) -> Vec<StoreAction> {
        match self {
            Self::App(id) => vec![StoreAction::Open(id.clone())],
            Self::Remove(id) => vec![
                StoreAction::Open(id.clone()),
                StoreAction::Remove(id.clone()),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_and_remove_uris_open_the_page() {
        assert_eq!(
            parse("store://app/km-office"),
            Some(DeepLink::App("km-office".into()))
        );
        assert_eq!(
            parse("store://app/org.ultar.files?action=remove"),
            Some(DeepLink::Remove("org.ultar.files".into()))
        );
        assert_eq!(
            DeepLink::App("x".into()).actions(),
            vec![StoreAction::Open("x".into())]
        );
        assert_eq!(
            DeepLink::Remove("x".into()).actions(),
            vec![
                StoreAction::Open("x".into()),
                StoreAction::Remove("x".into())
            ]
        );
    }

    #[test]
    fn malformed_or_unsafe_uris_open_nothing() {
        for bad in [
            "",
            "store://",
            "store://app/",
            "store://app/../etc",
            "store://app/a/b",
            "store://app/.hidden",
            "https://app/x",
            "settings://network",
            "store://app/x\n",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
        // An unknown query still opens the page (only action=remove is special).
        assert_eq!(
            parse("store://app/x?action=open"),
            Some(DeepLink::App("x".into()))
        );
    }
}
