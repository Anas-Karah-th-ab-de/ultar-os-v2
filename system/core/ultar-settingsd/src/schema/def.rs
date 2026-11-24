//! The declaration of one setting.

use super::area::{Domain, Panel};
use super::key::KeyTemplate;
use super::types::{TextFormat, ValueType};
use crate::value::Value;
use ultar_profile::{Capability, CapabilitySet};

/// How dangerous a change is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    /// Takes effect at once and stays.
    Safe,
    /// Applied provisionally; taken back unless confirmed within `seconds`.
    ConfirmRevert {
        /// Time to confirm.
        seconds: u32,
    },
    /// Only identities with administrator rights may change it.
    AdminOnly,
}

/// Where a value is remembered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    /// One value for the device.
    System,
    /// One value per user.
    User,
    /// One value per combination of connected displays.
    DisplayCombination,
}

/// What the `<id>` segment of a key stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceKind {
    /// The key has no instance segment.
    None,
    /// One key per connected display.
    Display,
    /// One key per app.
    App,
}

/// Where the default comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum DefaultValue {
    /// Always this value.
    Static(Value),
    /// Asked from the [`super::DefaultProvider`]; `fallback` when the
    /// provider has no answer.
    Computed {
        /// Value used when the device gives no answer.
        fallback: Value,
    },
}

impl DefaultValue {
    /// The static value or the fallback.
    pub const fn base(&self) -> &Value {
        match self {
            DefaultValue::Static(value) | DefaultValue::Computed { fallback: value } => value,
        }
    }
}

/// One declared setting.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingDef {
    /// Key template, e.g. `display.<id>.scale`.
    pub key: &'static str,
    /// Panel it is shown in.
    pub panel: Panel,
    /// Component that puts it into effect.
    pub domain: Domain,
    /// Type and limits.
    pub value_type: ValueType,
    /// Default.
    pub default: DefaultValue,
    /// Capabilities the device needs for this setting to exist.
    pub requires: CapabilitySet,
    /// Risk class.
    pub risk: Risk,
    /// Where the value is remembered.
    pub scope: Scope,
    /// What the `<id>` segment stands for.
    pub instance: InstanceKind,
}

impl SettingDef {
    /// A setting with the defaults: user scope, safe, no capabilities.
    pub fn new(key: &'static str, panel: Panel, value_type: ValueType, default: Value) -> Self {
        let has_instance = key.split('.').any(|s| s == super::key::INSTANCE_MARKER);
        let instance = match (has_instance, panel) {
            (false, _) => InstanceKind::None,
            (true, Panel::Display) => InstanceKind::Display,
            (true, _) => InstanceKind::App,
        };
        Self {
            key,
            panel,
            domain: panel.domain(),
            value_type,
            default: DefaultValue::Static(default),
            requires: CapabilitySet::EMPTY,
            risk: Risk::Safe,
            scope: Scope::User,
            instance,
        }
    }

    /// An on/off switch.
    pub fn flag(key: &'static str, panel: Panel, default: bool) -> Self {
        Self::new(key, panel, ValueType::Bool, Value::Bool(default))
    }

    /// A whole number in `range` (min, max, step).
    pub fn int(key: &'static str, panel: Panel, range: (i64, i64, i64), default: i64) -> Self {
        let (min, max, step) = range;
        Self::new(
            key,
            panel,
            ValueType::Int { min, max, step },
            Value::Int(default),
        )
    }

    /// A fractional number in `range` (min, max, step).
    pub fn float(key: &'static str, panel: Panel, range: (f64, f64, f64), default: f64) -> Self {
        let (min, max, step) = range;
        let value_type = ValueType::Float { min, max, step };
        Self::new(key, panel, value_type, Value::Float(default))
    }

    /// One of `choices`.
    pub fn choice(
        key: &'static str,
        panel: Panel,
        choices: &'static [&'static str],
        default: &str,
    ) -> Self {
        Self::new(key, panel, ValueType::Enum(choices), Value::text(default))
    }

    /// Text in a format.
    pub fn text(key: &'static str, panel: Panel, format: TextFormat, default: &str) -> Self {
        Self::new(key, panel, ValueType::Text(format), Value::text(default))
    }

    /// A list of texts in a format, empty by default.
    pub fn list(key: &'static str, panel: Panel, item: TextFormat, max_len: usize) -> Self {
        let value_type = ValueType::List { item, max_len };
        Self::new(key, panel, value_type, Value::List(Vec::new()))
    }

    /// Requires capabilities.
    pub fn needs(mut self, capabilities: &[Capability]) -> Self {
        self.requires = self.requires.union(CapabilitySet::of(capabilities));
        self
    }

    /// Sets the risk class.
    pub fn risk(mut self, risk: Risk) -> Self {
        self.risk = risk;
        self
    }

    /// Sets the scope.
    pub fn scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// Sets the applying component when it differs from the panel's.
    pub fn domain(mut self, domain: Domain) -> Self {
        self.domain = domain;
        self
    }

    /// Makes the default "computed from the device", keeping the declared
    /// value as fallback.
    pub fn computed(mut self) -> Self {
        self.default = DefaultValue::Computed {
            fallback: self.default.base().clone(),
        };
        self
    }

    fn i18n_stem(&self) -> String {
        match KeyTemplate::parse(self.key) {
            Some(template) => template.stem(),
            None => self.key.to_owned(),
        }
    }

    /// i18n key of the label.
    pub fn label_key(&self) -> String {
        format!("settings.{}.label", self.i18n_stem())
    }

    /// i18n key of the description.
    pub fn description_key(&self) -> String {
        format!("settings.{}.desc", self.i18n_stem())
    }

    /// i18n key of the label of one choice of an enum setting.
    pub fn choice_key(&self, choice: &str) -> String {
        format!("settings.{}.choice.{choice}", self.i18n_stem())
    }

    /// Every i18n key this setting needs.
    pub fn i18n_keys(&self) -> Vec<String> {
        let mut keys = vec![self.label_key(), self.description_key()];
        if let ValueType::Enum(choices) = self.value_type {
            keys.extend(choices.iter().map(|choice| self.choice_key(choice)));
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_profile::PanelId;

    #[test]
    fn panels_have_distinct_ids_and_parse_back() {
        for panel in Panel::ALL {
            assert_eq!(Panel::parse(panel.id()), Some(panel));
            assert_eq!(panel.title_key(), format!("settings.panel.{}", panel.id()));
        }
        assert_eq!(Panel::parse("nope"), None);
        assert!(Panel::DesktopBar.needs_display());
        assert!(!Panel::Power.needs_display());
        assert_eq!(Panel::Region.profile_panel(), Some(PanelId::Language));
        assert_eq!(Panel::Notifications.profile_panel(), None);
        let mut names: Vec<&str> = Domain::ALL.iter().map(|d| d.as_str()).collect();
        names.dedup();
        assert_eq!(names.len(), Domain::ALL.len());
    }

    #[test]
    fn builders_set_what_they_say() {
        let scale = SettingDef::int("display.<id>.scale", Panel::Display, (100, 300, 25), 100)
            .risk(Risk::ConfirmRevert { seconds: 15 })
            .scope(Scope::DisplayCombination)
            .computed();
        assert_eq!(scale.instance, InstanceKind::Display);
        assert_eq!(scale.domain, Domain::Display);
        assert_eq!(scale.default.base(), &Value::Int(100));
        assert!(matches!(scale.default, DefaultValue::Computed { .. }));
        assert_eq!(scale.label_key(), "settings.display.scale.label");
        assert_eq!(scale.description_key(), "settings.display.scale.desc");

        let app = SettingDef::flag("notifications.app.<id>.sound", Panel::Notifications, true);
        assert_eq!(app.instance, InstanceKind::App);
        let battery = SettingDef::flag("power.show_battery_percent", Panel::Power, false)
            .needs(&[Capability::Battery])
            .domain(Domain::Shell);
        assert!(battery.requires.contains(Capability::Battery));
        assert_eq!(battery.domain, Domain::Shell);
        assert_eq!(battery.instance, InstanceKind::None);
    }

    #[test]
    fn i18n_keys_cover_choices() {
        let theme = SettingDef::choice(
            "appearance.theme",
            Panel::Appearance,
            &["dark", "light"],
            "dark",
        );
        assert_eq!(
            theme.i18n_keys(),
            [
                "settings.appearance.theme.label",
                "settings.appearance.theme.desc",
                "settings.appearance.theme.choice.dark",
                "settings.appearance.theme.choice.light",
            ]
        );
        let rate = SettingDef::float("a.rate", Panel::Display, (0.0, 1.0, 0.0), 0.5);
        assert_eq!(rate.i18n_keys().len(), 2);
        let text = SettingDef::text("a.t", Panel::Region, TextFormat::Currency, "SYP");
        let list = SettingDef::list("a.l", Panel::Display, TextFormat::Name, 4);
        assert_eq!(text.default.base(), &Value::text("SYP"));
        assert_eq!(list.default.base(), &Value::List(Vec::new()));
    }
}
