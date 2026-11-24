//! The areas of the sidebar (boards: nav "أقسام الإعدادات", 32 entries) and what each opens.

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

use PageKind::{NoService as N, Panel as P};

/// Every area in board order.
pub const AREAS: [Area; 33] = [
    a("wifi", IconId::Wifi, PageKind::Status(StatusKind::Network)),
    a("cellular", IconId::Cell, N),
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
    a("search", IconId::Search, N),
    a("apps", IconId::Apps, N),
    a("mouse", IconId::Mouse, N),
    a("keyboard", IconId::Keyboard, N),
    a("printers", IconId::Printer, N),
    a("colour", IconId::Colour, N),
    a("accessibility", IconId::Access, N),
    a("privacy", IconId::Shield, P("privacy")),
    a("sharing", IconId::Sharing, N),
    a("accounts", IconId::User, PageKind::Account),
    a("users", IconId::Users, N),
    a("region", IconId::Language, P("region")),
    a("datetime", IconId::Clock, P("date_time")),
    a("storage", IconId::Disk, N),
    a("drivers", IconId::Chip, PageKind::Drivers),
    a("updates", IconId::Update, N),
    a("backup", IconId::Backup, N),
    a("mesh", IconId::Mesh, N),
    a("assistant", IconId::Spark, N),
    a("cells", IconId::Cells, N),
    a("emergency", IconId::Sos, N),
    a("about", IconId::Device, PageKind::About),
];

/// Areas a wide sidebar shows (the mobile network only exists on compact devices).
pub fn wide() -> Vec<usize> {
    (0..AREAS.len())
        .filter(|&i| AREAS[i].id != "cellular")
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
        assert_eq!(wide().len(), 32);
        let c = compact();
        assert_eq!(c.len(), 33);
        assert_eq!(AREAS[c[1]].id, "cellular");
        assert_eq!(index_of("nope"), None);
        assert_eq!(AREAS[0].label_key(), "app.settings.area.wifi");
    }
}
