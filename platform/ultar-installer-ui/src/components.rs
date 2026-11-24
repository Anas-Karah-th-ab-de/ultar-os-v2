//! Step 8: the components (DESKTOP-SPEC §6 no. 8, §7.2) and their rules: Firefox needs the
//! Linux programs; deselecting the Linux programs deselects Firefox (after a hint).

use std::collections::BTreeSet;
use ultar_ui::icon::IconId;

/// A component of the installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Component {
    /// Office tools.
    Office,
    /// Developer tools.
    Developer,
    /// Media.
    Media,
    /// Communication.
    Communication,
    /// Mesh.
    Mesh,
    /// Assistant and local AI.
    Assistant,
    /// Linux programs (.deb in an isolated area).
    Linux,
    /// Firefox.
    Firefox,
    /// Windows programs.
    Windows,
    /// Android apps.
    Android,
    /// Containers and server.
    Containers,
    /// Remote access.
    Remote,
}

impl Component {
    /// Board order (two columns: start, end, start, end …).
    pub const ALL: [Component; 12] = [
        Component::Office,
        Component::Developer,
        Component::Media,
        Component::Communication,
        Component::Mesh,
        Component::Assistant,
        Component::Linux,
        Component::Firefox,
        Component::Windows,
        Component::Android,
        Component::Containers,
        Component::Remote,
    ];

    /// Stable id.
    pub fn id(self) -> &'static str {
        match self {
            Component::Office => "office",
            Component::Developer => "developer",
            Component::Media => "media",
            Component::Communication => "communication",
            Component::Mesh => "mesh",
            Component::Assistant => "assistant",
            Component::Linux => "linux",
            Component::Firefox => "firefox",
            Component::Windows => "windows",
            Component::Android => "android",
            Component::Containers => "containers",
            Component::Remote => "remote",
        }
    }

    /// Icon (ICONS.md).
    pub fn icon(self) -> IconId {
        match self {
            Component::Office => IconId::Bag,
            Component::Developer => IconId::Keyboard,
            Component::Media => IconId::Video,
            Component::Communication => IconId::Message,
            Component::Mesh => IconId::Mesh,
            Component::Assistant => IconId::Spark,
            Component::Linux => IconId::Apps,
            Component::Firefox => IconId::Network,
            Component::Windows => IconId::Grid,
            Component::Android => IconId::Phone,
            Component::Containers => IconId::Cells,
            Component::Remote => IconId::ScreenShare,
        }
    }
}

/// Selection size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// System and basic apps only.
    Base,
    /// Base plus office, media, communication and Firefox.
    Extended,
    /// One by one.
    #[default]
    Custom,
}

impl Scope {
    /// Board order.
    pub const ALL: [Scope; 3] = [Scope::Base, Scope::Extended, Scope::Custom];

    /// Stable id.
    pub fn id(self) -> &'static str {
        match self {
            Scope::Base => "base",
            Scope::Extended => "extended",
            Scope::Custom => "custom",
        }
    }
}

/// The preselection (board: office, developer, assistant, Linux programs, Firefox).
pub fn defaults() -> BTreeSet<Component> {
    [
        Component::Office,
        Component::Developer,
        Component::Assistant,
        Component::Linux,
        Component::Firefox,
    ]
    .into_iter()
    .collect()
}

/// The components a scope fills in.
pub fn of_scope(scope: Scope, current: &BTreeSet<Component>) -> BTreeSet<Component> {
    match scope {
        Scope::Base => BTreeSet::new(),
        Scope::Extended => [
            Component::Office,
            Component::Media,
            Component::Communication,
            Component::Linux,
            Component::Firefox,
        ]
        .into_iter()
        .collect(),
        Scope::Custom => current.clone(),
    }
}

/// What a toggle changed besides the component itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Nothing else.
    None,
    /// Firefox selected the Linux programs too.
    AddedLinux,
    /// Deselecting the Linux programs deselected Firefox (show a hint).
    RemovedFirefox,
}

/// Selects or deselects a component with the dependency rule; the scope becomes Custom.
pub fn toggle(set: &mut BTreeSet<Component>, c: Component, on: bool) -> Effect {
    if on {
        set.insert(c);
        if c == Component::Firefox && set.insert(Component::Linux) {
            return Effect::AddedLinux;
        }
        return Effect::None;
    }
    set.remove(&c);
    if c == Component::Linux && set.remove(&Component::Firefox) {
        return Effect::RemovedFirefox;
    }
    Effect::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firefox_needs_linux() {
        let mut s = BTreeSet::new();
        assert_eq!(toggle(&mut s, Component::Firefox, true), Effect::AddedLinux);
        assert!(s.contains(&Component::Linux));
        assert_eq!(toggle(&mut s, Component::Firefox, true), Effect::None);
        assert_eq!(
            toggle(&mut s, Component::Linux, false),
            Effect::RemovedFirefox
        );
        assert!(s.is_empty());
        assert_eq!(toggle(&mut s, Component::Media, false), Effect::None);
    }

    #[test]
    fn scopes_fill_the_list() {
        let d = defaults();
        assert!(of_scope(Scope::Base, &d).is_empty());
        assert!(of_scope(Scope::Extended, &d).contains(&Component::Firefox));
        assert_eq!(of_scope(Scope::Custom, &d), d);
        let ids: BTreeSet<_> = Component::ALL.iter().map(|c| c.id()).collect();
        assert_eq!(ids.len(), 12);
        assert_eq!(Scope::ALL.map(Scope::id), ["base", "extended", "custom"]);
    }
}
