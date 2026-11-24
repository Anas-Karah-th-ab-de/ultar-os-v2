//! The areas of the sidebar (boards: nav "أقسام الإعدادات", 32 entries), what each opens, and
//! the groups the wide sidebar shows them in.

use ultar_ui::icon::IconId;

/// What the content pane shows for an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageKind {
    /// Board *SettingsDisplay*.
    Display,
    /// Board *SettingsDrivers*.
    Drivers,
    /// Board *Account* (Settings > Accounts > Ultar account).
    Account,
    /// The rows of one `settingsd` panel (generic editor).
    Panel(&'static str),
    /// Read-only state of a service (network, Bluetooth, sound).
    Status(StatusKind),
    /// Boards *SettingsMesh* and *MeshDevice*.
    Mesh,
    /// About this system (BRAND.md §4): product, version, build, company, developer, website,
    /// contact, licences.
    About,
    /// No service offers settings for this area in this version (empty state with reason).
    NoService,
}

/// The service a status page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusKind {
    /// `net.state`: Wi-Fi and the wired connection.
    Network,
    /// `bluetooth.state`.
    Bluetooth,
    /// `audio.state`.
    Sound,
}

/// One sidebar area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    /// Stable id; label key `app.settings.area.<id>`.
    pub id: &'static str,
    /// Icon (ICONS.md).
    pub icon: IconId,
    /// Content.
    pub page: PageKind,
}

impl Area {
    /// Catalog key of the label.
    pub fn label_key(&self) -> String {
        format!("app.settings.area.{}", self.id)
    }
}

const fn a(id: &'static str, icon: IconId, page: PageKind) -> Area {
    Area { id, icon, page }
}

use PageKind::Panel as P;

/// Every area in board order.
pub const AREAS: [Area; 33] = [
    a("wifi", IconId::Wifi, PageKind::Status(StatusKind::Network)),
    a("cellular", IconId::Cell, P("mobile")),
    a(
        "network",
        IconId::Network,
        PageKind::Status(StatusKind::Network),
    ),
    a(
        "bluetooth",
        IconId::Bluetooth,
        PageKind::Status(StatusKind::Bluetooth),
    ),
    a("displays", IconId::Monitor, PageKind::Display),
    a("sound", IconId::Volume, PageKind::Status(StatusKind::Sound)),
    a("power", IconId::Battery, P("power")),
    a("desktop", IconId::Dock, P("desktop_bar")),
    a("windows", IconId::Workspaces, P("windows")),
    a("appearance", IconId::Palette, P("appearance")),
    a("notifications", IconId::Bell, P("notifications")),
    a("search", IconId::Search, P("search")),
    a("apps", IconId::Apps, P("apps")),
    a("mouse", IconId::Mouse, P("pointer")),
    a("keyboard", IconId::Keyboard, P("keyboard")),
    a("printers", IconId::Printer, P("printers")),
    a("colour", IconId::Colour, P("colour")),
    a("accessibility", IconId::Access, P("accessibility")),
    a("privacy", IconId::Shield, P("privacy")),
    a("sharing", IconId::Sharing, P("sharing")),
    a("accounts", IconId::User, PageKind::Account),
    a("users", IconId::Users, P("users")),
    a("region", IconId::Language, P("region")),
    a("datetime", IconId::Clock, P("date_time")),
    a("storage", IconId::Disk, P("storage")),
    a("drivers", IconId::Chip, PageKind::Drivers),
    a("updates", IconId::Update, P("updates")),
    a("backup", IconId::Backup, P("backup")),
    a("mesh", IconId::Mesh, PageKind::Mesh),
    a("assistant", IconId::Spark, P("assistant")),
    a("cells", IconId::Cells, P("cells")),
    a("emergency", IconId::Sos, P("emergency")),
    a("about", IconId::Device, PageKind::About),
];

/// A heading of the wide sidebar and the areas under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    /// Stable id; label key `app.settings.group.<id>`.
    pub id: &'static str,
    /// Area ids in the order shown.
    pub areas: &'static [&'static str],
}

impl Group {
    /// Catalog key of the heading.
    pub fn label_key(&self) -> String {
        format!("app.settings.group.{}", self.id)
    }
}

/// The headings of the wide sidebar (owner's order of 24.11.2026; the boards still show one
/// flat list). Every area is in exactly one group.
pub const GROUPS: [Group; 6] = [
    Group {
        id: "connections",
        areas: &[
            "wifi",
            "network",
            "cellular",
            "bluetooth",
            "sharing",
            "mesh",
        ],
    },
    Group {
        id: "device",
        areas: &[
            "displays", "sound", "power", "mouse", "keyboard", "printers", "storage", "drivers",
        ],
    },
    Group {
        id: "personal",
        areas: &[
            "desktop",
            "windows",
            "appearance",
            "colour",
            "notifications",
            "search",
        ],
    },
    Group {
        id: "apps",
        areas: &["apps", "accounts", "users", "assistant"],
    },
    Group {
        id: "security",
        areas: &["privacy", "cells", "emergency"],
    },
    Group {
        id: "system",
        areas: &[
            "region",
            "datetime",
            "updates",
            "backup",
            "accessibility",
            "about",
        ],
    },
];

/// The group an area belongs to (index into [`GROUPS`]).
pub fn group_of(area: usize) -> Option<usize> {
    let id = AREAS.get(area)?.id;
    GROUPS.iter().position(|g| g.areas.contains(&id))
}

/// Areas a wide sidebar shows, group after group (the shell hides those whose hardware is
/// absent: the mobile network without a modem, mouse and touchpad without a pointer).
pub fn wide() -> Vec<usize> {
    GROUPS
        .iter()
        .flat_map(|g| g.areas.iter())
        .filter_map(|id| index_of(id))
        .collect()
}

/// Areas in the order of the compact list (board *CompactSettings*), the rest after them.
pub fn compact() -> Vec<usize> {
    const FIRST: [&str; 10] = [
        "wifi",
        "cellular",
        "bluetooth",
        "displays",
        "sound",
        "power",
        "notifications",
        "appearance",
        "apps",
        "privacy",
    ];
    let mut order: Vec<usize> = FIRST.iter().filter_map(|id| index_of(id)).collect();
    let rest: Vec<usize> = (0..AREAS.len()).filter(|i| !order.contains(i)).collect();
    order.extend(rest);
    order
}

/// Index of an area by id.
pub fn index_of(id: &str) -> Option<usize> {
    AREAS.iter().position(|a| a.id == id)
}

/// Index of the area that shows a page.
pub fn index_of_page(page: PageKind) -> Option<usize> {
    AREAS.iter().position(|a| a.page == page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_pages_found() {
        let mut ids: Vec<_> = AREAS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), AREAS.len());
        assert_eq!(index_of("displays"), Some(4));
        assert_eq!(index_of_page(PageKind::Drivers), Some(25));
        assert_eq!(index_of_page(PageKind::Account), Some(20));
        assert_eq!(wide().len(), 33);
        let c = compact();
        assert_eq!(c.len(), 33);
        assert_eq!(AREAS[c[1]].id, "cellular");
        assert_eq!(index_of("nope"), None);
        assert_eq!(AREAS[0].label_key(), "app.settings.area.wifi");
    }

    #[test]
    fn every_area_is_in_exactly_one_group_and_has_a_page() {
        for (i, area) in AREAS.iter().enumerate() {
            let n = GROUPS.iter().filter(|g| g.areas.contains(&area.id)).count();
            assert_eq!(n, 1, "{}", area.id);
            assert!(group_of(i).is_some());
            assert_ne!(area.page, PageKind::NoService, "{}", area.id);
        }
        let listed: usize = GROUPS.iter().map(|g| g.areas.len()).sum();
        assert_eq!(listed, AREAS.len());
        // The wide sidebar follows the groups: Wi-Fi first, About last.
        let w = wide();
        assert_eq!(AREAS[w[0]].id, "wifi");
        assert_eq!(AREAS[*w.last().unwrap()].id, "about");
        let groups: Vec<usize> = w.iter().filter_map(|&i| group_of(i)).collect();
        assert!(groups.windows(2).all(|p| p[0] <= p[1]));
        assert_eq!(GROUPS[0].label_key(), "app.settings.group.connections");
    }
}
