//! Search over the catalogue (DESKTOP-SPEC §7.3 "Suche"): name, description and file
//! type; Arabic tolerant (no diacritics, alef/yaa forms folded), Latin insensitive.
//! A query starting with `.` searches file types (`.xlsx` finds the spreadsheet app).

use crate::catalog::{Catalog, CatalogApp};
use ultar_i18n::normalize_for_search;

/// One search hit and how strongly it matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// The name starts with the query.
    NameStart,
    /// The name contains the query.
    Name,
    /// A file type matches (`.xlsx`).
    FileType,
    /// The category matches.
    Category,
    /// The summary or the description matches.
    Text,
    /// The publisher matches.
    Publisher,
    /// The id matches (exact ids sort first).
    Id,
}

/// A hit: the entry and its rank (lower is better).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit<'a> {
    /// The matched entry.
    pub app: &'a CatalogApp,
    /// Match strength.
    pub kind: MatchKind,
    /// Whether the id matched exactly.
    pub exact_id: bool,
}

impl MatchKind {
    /// Ordering weight of the kind.
    const fn weight(self) -> u8 {
        match self {
            Self::NameStart => 0,
            Self::Name => 1,
            Self::Id => 2,
            Self::FileType => 3,
            Self::Category => 4,
            Self::Text => 5,
            Self::Publisher => 6,
        }
    }
}

/// Lower-case, Arabic-folded form used for every comparison.
fn fold(text: &str) -> String {
    normalize_for_search(&text.to_lowercase())
}

/// True when the folded ` haystack` contains the folded `needle` (already folded).
fn contains(haystack: &str, needle: &str) -> bool {
    let h = fold(haystack);
    h.contains(needle)
}

/// Search the catalogue; results are ranked, ids unique, empty query gives nothing.
pub fn search<'a>(catalog: &'a Catalog, query: &str) -> Vec<Hit<'a>> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let needle = fold(q);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit<'a>> = Vec::new();
    for app in &catalog.apps {
        let starts = app
            .name
            .texts()
            .into_iter()
            .any(|t| fold(t).starts_with(&needle));
        let name = app.name.texts().into_iter().any(|t| contains(t, &needle));
        let body = app
            .summary
            .texts()
            .into_iter()
            .chain(app.description.texts())
            .any(|t| contains(t, &needle));
        let kind = if starts {
            MatchKind::NameStart
        } else if name {
            MatchKind::Name
        } else if fold(&app.id) == needle {
            MatchKind::Id
        } else if app.file_types.iter().any(|ft| contains(ft, &needle)) {
            MatchKind::FileType
        } else if contains(&app.category, &needle) {
            MatchKind::Category
        } else if body {
            MatchKind::Text
        } else if contains(&app.publisher, &needle) {
            MatchKind::Publisher
        } else {
            continue;
        };
        hits.push(Hit {
            app,
            kind,
            exact_id: fold(&app.id) == needle,
        });
    }
    hits.sort_by(|a, b| {
        b.exact_id
            .cmp(&a.exact_id)
            .then(a.kind.weight().cmp(&b.kind.weight()))
            .then_with(|| fold(&a.app.name.ar).cmp(&fold(&b.app.name.ar)))
    });
    hits
}

/// The hits for the current query (views and tests).
pub fn visible_results(store: &crate::state::Store) -> Vec<Hit<'_>> {
    if store.query.is_empty() {
        return Vec::new();
    }
    match &store.catalog.data {
        Some(c) => search(c, &store.query),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Loc, Source};

    fn app(id: &str, name: &str, fts: &[&str]) -> CatalogApp {
        let loc = |s: &str| Loc {
            ar: s.into(),
            en: s.into(),
            de: None,
        };
        CatalogApp {
            id: id.into(),
            name: loc(name),
            summary: loc("plain text body"),
            description: loc("does nothing special"),
            publisher: "KM Code Craft".into(),
            category: "office".into(),
            icon: "office".into(),
            source: Source::Ultar,
            version: None,
            size_download: None,
            size_installed: None,
            file_types: fts.iter().map(|s| (*s).to_string()).collect(),
            permissions: Vec::new(),
            requires: Vec::new(),
            package: None,
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            format_version: crate::catalog::FORMAT_VERSION,
            generated: None,
            apps: vec![
                app(
                    "km-office",
                    "Office tools",
                    &[".docx", ".xlsx", ".pptx", ".pdf"],
                ),
                app("coder", "Coder", &[]),
                app("media", "Media", &[".mp3"]),
            ],
        }
    }

    #[test]
    fn file_type_query_finds_the_spreadsheet_app() {
        let cat = catalog();
        let hits = search(&cat, ".xlsx");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].app.id, "km-office");
        assert_eq!(hits[0].kind, MatchKind::FileType);
    }

    #[test]
    fn empty_and_blank_queries_return_nothing() {
        assert!(search(&catalog(), "").is_empty());
        assert!(search(&catalog(), "   ").is_empty());
    }

    #[test]
    fn name_start_outranks_name_contains() {
        let mut c = catalog();
        c.apps.push(app("office-extra", "My Office extra", &[]));
        let hits = search(&c, "office");
        assert_eq!(hits[0].app.id, "km-office");
        assert_eq!(hits[0].kind, MatchKind::NameStart);
        assert_eq!(hits[1].kind, MatchKind::Name);
    }

    #[test]
    fn description_and_publisher_match() {
        let cat = catalog();
        let hits = search(&cat, "special");
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().all(|h| h.kind == MatchKind::Text));
        let hits = search(&cat, "craft");
        assert!(hits.iter().all(|h| h.kind == MatchKind::Publisher));
    }
}
