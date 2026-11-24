//! The named areas of the user's data (APP-RULES §4/§5): the app never sees raw paths.

/// One area the cell may grant.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AreaId {
    /// The user's home folder.
    Home,
    /// `documents`.
    Documents,
    /// `downloads`.
    Downloads,
    /// `media:pictures`.
    Pictures,
    /// `media:music`.
    Music,
    /// `media:videos`.
    Videos,
    /// `desktop` (the icons on the desktop).
    Desktop,
    /// `trash`: where deleted items wait.
    Trash,
    /// A mounted removable drive (by its folder name under `removable`).
    Drive(String),
}

/// The fixed areas in sidebar order.
pub const AREAS: [AreaId; 8] = [
    AreaId::Home,
    AreaId::Documents,
    AreaId::Downloads,
    AreaId::Pictures,
    AreaId::Music,
    AreaId::Videos,
    AreaId::Desktop,
    AreaId::Trash,
];

impl AreaId {
    /// Name of the area in manifests and records (`media:pictures`).
    pub fn name(&self) -> String {
        match self {
            AreaId::Drive(d) => format!("removable/{d}"),
            other => other.fixed_name().to_owned(),
        }
    }

    fn fixed_name(&self) -> &'static str {
        match self {
            AreaId::Home => "home",
            AreaId::Documents => "documents",
            AreaId::Downloads => "downloads",
            AreaId::Pictures => "media-pictures",
            AreaId::Music => "media-music",
            AreaId::Videos => "media-videos",
            AreaId::Desktop => "desktop",
            AreaId::Trash => "trash",
            AreaId::Drive(_) => "removable",
        }
    }

    /// Folder name of the area under the cell's area root (`media-pictures`).
    pub fn dir_name(&self) -> String {
        self.name()
    }

    /// Reads [`AreaId::name`]'s form.
    pub fn parse(text: &str) -> Option<AreaId> {
        if let Some(drive) = text.strip_prefix("removable/") {
            return super::check_name(drive)
                .ok()
                .map(|_| AreaId::Drive(drive.to_owned()));
        }
        AREAS.iter().find(|a| a.fixed_name() == text).cloned()
    }

    /// i18n key of the area's name in the sidebar (drives show their own label).
    pub fn label_key(&self) -> &'static str {
        match self {
            AreaId::Home => "files.place.home",
            AreaId::Documents => "files.place.documents",
            AreaId::Downloads => "files.place.downloads",
            AreaId::Pictures => "files.place.pictures",
            AreaId::Music => "files.place.music",
            AreaId::Videos => "files.place.videos",
            AreaId::Desktop => "files.place.desktop",
            AreaId::Trash => "files.place.trash",
            AreaId::Drive(_) => "files.place.drive",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for a in AREAS {
            assert_eq!(AreaId::parse(&a.name()), Some(a.clone()));
            assert!(a.label_key().starts_with("files.place."));
        }
        let d = AreaId::Drive("STICK".into());
        assert_eq!(AreaId::parse(&d.name()), Some(d));
        assert_eq!(AreaId::parse("removable/.."), None);
        assert_eq!(AreaId::parse("etc"), None);
    }
}
