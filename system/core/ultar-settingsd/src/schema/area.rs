//! The panels of the settings app and the domains (the components that put settings into
//! effect).

use ultar_profile::PanelId;

/// Which component puts a setting into effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Domain {
    /// Outputs, modes, night light, colour management.
    Display,
    /// Theme, colours, wallpaper, sizes.
    Appearance,
    /// Bar, dock, desktop icons.
    Shell,
    /// Window manager and workspaces.
    Windows,
    /// Notification service.
    Notifications,
    /// Power service.
    Power,
    /// Language and formats.
    Locale,
    /// Clock and time zone.
    Time,
    /// Privacy switches, lock, clean-up.
    Privacy,
    /// Ultar account services.
    Account,
    /// Toolkit, shell and input helpers for accessibility.
    Accessibility,
    /// Mobile network (modem service).
    Modem,
    /// System search (desktop shell).
    Search,
    /// Default apps, autostart, app rights (launcher).
    Apps,
    /// Pointer, touchpad and keyboard (input seat).
    Input,
    /// Printers and scanners.
    Printing,
    /// Device name, file, media, screen and SSH sharing in the local network.
    Sharing,
    /// Users and sign-in (account service).
    Users,
    /// Drives, clean-up, space warnings (volume service).
    Storage,
    /// System and app updates (updater).
    Update,
    /// Backups.
    Backup,
    /// Mesh of the user's devices (verbund service).
    Mesh,
    /// The assistant (local language model).
    Assistant,
    /// Cells and containers.
    Cells,
    /// Emergency call, contact, medical details, SOS.
    Emergency,
}

impl Domain {
    /// Every domain.
    pub const ALL: [Domain; 25] = [
        Domain::Display,
        Domain::Appearance,
        Domain::Shell,
        Domain::Windows,
        Domain::Notifications,
        Domain::Power,
        Domain::Locale,
        Domain::Time,
        Domain::Privacy,
        Domain::Account,
        Domain::Accessibility,
        Domain::Modem,
        Domain::Search,
        Domain::Apps,
        Domain::Input,
        Domain::Printing,
        Domain::Sharing,
        Domain::Users,
        Domain::Storage,
        Domain::Update,
        Domain::Backup,
        Domain::Mesh,
        Domain::Assistant,
        Domain::Cells,
        Domain::Emergency,
    ];

    /// Stable lower-case name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Domain::Display => "display",
            Domain::Appearance => "appearance",
            Domain::Shell => "shell",
            Domain::Windows => "windows",
            Domain::Notifications => "notifications",
            Domain::Power => "power",
            Domain::Locale => "locale",
            Domain::Time => "time",
            Domain::Privacy => "privacy",
            Domain::Account => "account",
            Domain::Accessibility => "accessibility",
            Domain::Modem => "modem",
            Domain::Search => "search",
            Domain::Apps => "apps",
            Domain::Input => "input",
            Domain::Printing => "printing",
            Domain::Sharing => "sharing",
            Domain::Users => "users",
            Domain::Storage => "storage",
            Domain::Update => "update",
            Domain::Backup => "backup",
            Domain::Mesh => "mesh",
            Domain::Assistant => "assistant",
            Domain::Cells => "cells",
            Domain::Emergency => "emergency",
        }
    }
}

/// Panel of the settings app a setting is shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Panel {
    /// Displays: arrangement, mode, per-display values, night light.
    Display,
    /// Theme, accent colour, wallpaper, text and icon size.
    Appearance,
    /// Desktop bar, dock and desktop icons.
    DesktopBar,
    /// Windows and workspaces.
    Windows,
    /// Notifications.
    Notifications,
    /// Power.
    Power,
    /// Region and language.
    Region,
    /// Date and time.
    DateTime,
    /// Privacy and security.
    Privacy,
    /// Ultar account.
    Account,
    /// Mobile network: data, roaming, network type, access point.
    Mobile,
    /// System search: sources, places, history.
    Search,
    /// Default apps, autostart, removable media, rights per app.
    Apps,
    /// Mouse and touchpad.
    Pointer,
    /// Keyboard: input languages, repeat, on-screen keyboard.
    Keyboard,
    /// Printers and scanners.
    Printers,
    /// Colour management of the displays.
    Colour,
    /// Accessibility: seeing, hearing, typing, pointing.
    Accessibility,
    /// Sharing in the local network: device name, files, media, screen, SSH.
    Sharing,
    /// Users and sign-in.
    Users,
    /// Storage: clean-up, space warnings, drives.
    Storage,
    /// Updates: automatic, time window, channel.
    Updates,
    /// Backup: target, folders, schedule, encryption.
    Backup,
    /// Mesh of the user's devices.
    Mesh,
    /// The assistant.
    Assistant,
    /// Cells and containers.
    Cells,
    /// Emergency.
    Emergency,
}

impl Panel {
    /// Every panel in display order.
    pub const ALL: [Panel; 27] = [
        Panel::Display,
        Panel::Appearance,
        Panel::DesktopBar,
        Panel::Windows,
        Panel::Notifications,
        Panel::Power,
        Panel::Region,
        Panel::DateTime,
        Panel::Privacy,
        Panel::Account,
        Panel::Mobile,
        Panel::Search,
        Panel::Apps,
        Panel::Pointer,
        Panel::Keyboard,
        Panel::Printers,
        Panel::Colour,
        Panel::Accessibility,
        Panel::Sharing,
        Panel::Users,
        Panel::Storage,
        Panel::Updates,
        Panel::Backup,
        Panel::Mesh,
        Panel::Assistant,
        Panel::Cells,
        Panel::Emergency,
    ];

    /// Stable identifier.
    pub const fn id(self) -> &'static str {
        match self {
            Panel::Display => "display",
            Panel::Appearance => "appearance",
            Panel::DesktopBar => "desktop_bar",
            Panel::Windows => "windows",
            Panel::Notifications => "notifications",
            Panel::Power => "power",
            Panel::Region => "region",
            Panel::DateTime => "date_time",
            Panel::Privacy => "privacy",
            Panel::Account => "account",
            Panel::Mobile => "mobile",
            Panel::Search => "search",
            Panel::Apps => "apps",
            Panel::Pointer => "pointer",
            Panel::Keyboard => "keyboard",
            Panel::Printers => "printers",
            Panel::Colour => "colour",
            Panel::Accessibility => "accessibility",
            Panel::Sharing => "sharing",
            Panel::Users => "users",
            Panel::Storage => "storage",
            Panel::Updates => "updates",
            Panel::Backup => "backup",
            Panel::Mesh => "mesh",
            Panel::Assistant => "assistant",
            Panel::Cells => "cells",
            Panel::Emergency => "emergency",
        }
    }

    /// Finds a panel by its identifier.
    pub fn parse(id: &str) -> Option<Panel> {
        Panel::ALL.into_iter().find(|panel| panel.id() == id)
    }

    /// i18n key of the panel title.
    pub fn title_key(self) -> String {
        format!("settings.panel.{}", self.id())
    }

    /// The panel of `ultar-profile` whose visibility rule applies, if the
    /// profile crate knows this panel.
    pub const fn profile_panel(self) -> Option<PanelId> {
        match self {
            Panel::Display => Some(PanelId::Display),
            Panel::Appearance => Some(PanelId::Appearance),
            Panel::DesktopBar
            | Panel::Windows
            | Panel::Notifications
            | Panel::Search
            | Panel::Printers
            | Panel::Colour
            | Panel::Accessibility
            | Panel::Sharing
            | Panel::Users
            | Panel::Backup
            | Panel::Mesh
            | Panel::Assistant
            | Panel::Cells
            | Panel::Emergency => None,
            Panel::Power => Some(PanelId::Power),
            Panel::Region => Some(PanelId::Language),
            Panel::DateTime => Some(PanelId::DateTime),
            Panel::Privacy => Some(PanelId::Security),
            Panel::Account => Some(PanelId::Accounts),
            Panel::Mobile => Some(PanelId::MobileNetwork),
            Panel::Apps => Some(PanelId::Apps),
            Panel::Pointer => Some(PanelId::Pointer),
            Panel::Keyboard => Some(PanelId::Keyboard),
            Panel::Storage => Some(PanelId::Storage),
            Panel::Updates => Some(PanelId::Updates),
        }
    }

    /// Panels the profile crate has no rule for but which only make sense
    /// with a display.
    pub const fn needs_display(self) -> bool {
        matches!(self, Panel::DesktopBar | Panel::Windows | Panel::Colour)
    }

    /// The component that puts this panel's settings into effect.
    pub const fn domain(self) -> Domain {
        match self {
            Panel::Display => Domain::Display,
            Panel::Appearance => Domain::Appearance,
            Panel::DesktopBar => Domain::Shell,
            Panel::Windows => Domain::Windows,
            Panel::Notifications => Domain::Notifications,
            Panel::Power => Domain::Power,
            Panel::Region => Domain::Locale,
            Panel::DateTime => Domain::Time,
            Panel::Privacy => Domain::Privacy,
            Panel::Account => Domain::Account,
            Panel::Mobile => Domain::Modem,
            Panel::Search => Domain::Search,
            Panel::Apps => Domain::Apps,
            Panel::Pointer => Domain::Input,
            Panel::Keyboard => Domain::Input,
            Panel::Printers => Domain::Printing,
            Panel::Colour => Domain::Display,
            Panel::Accessibility => Domain::Accessibility,
            Panel::Sharing => Domain::Sharing,
            Panel::Users => Domain::Users,
            Panel::Storage => Domain::Storage,
            Panel::Updates => Domain::Update,
            Panel::Backup => Domain::Backup,
            Panel::Mesh => Domain::Mesh,
            Panel::Assistant => Domain::Assistant,
            Panel::Cells => Domain::Cells,
            Panel::Emergency => Domain::Emergency,
        }
    }
}
