//! Schema registry: keys, types, defaults, risk classes, scopes, migrations.

mod def;
mod key;
mod migrate;
mod registry;
mod types;

pub use def::{DefaultValue, Domain, InstanceKind, Panel, Risk, Scope, SettingDef};
pub use key::{
    is_valid_key, is_valid_segment, sanitize_segment, KeyTemplate, INSTANCE_MARKER, MAX_KEY_LEN,
    MAX_SEGMENT_LEN,
};
pub use migrate::{migrate, Migration, MigrationStep};
pub use registry::{LookupError, Registry, RegistryBuilder, Resolved, SchemaError};
pub use types::{TextFormat, ValueError, ValueType};

use crate::value::Value;

/// Supplies defaults that depend on the device ("computed from device").
pub trait DefaultProvider: Send {
    /// The default for `def` (and the instance, for keys with `<id>`), or
    /// `None` when the device gives no answer; the declared fallback is
    /// used then. Answers that are not valid for the type are ignored too.
    fn computed_default(&self, def: &SettingDef, instance: Option<&str>) -> Option<Value>;
}

/// A provider without answers: every computed default is its fallback.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDefaults;

impl DefaultProvider for NoDefaults {
    fn computed_default(&self, _def: &SettingDef, _instance: Option<&str>) -> Option<Value> {
        None
    }
}

/// The default of a setting: the provider's answer when the declaration
/// asks for one and the answer is valid, else the declared value.
pub fn default_of(
    def: &SettingDef,
    instance: Option<&str>,
    provider: &dyn DefaultProvider,
) -> Value {
    match &def.default {
        DefaultValue::Static(value) => value.clone(),
        DefaultValue::Computed { fallback } => provider
            .computed_default(def, instance)
            .and_then(|value| def.value_type.accept(value).ok())
            .unwrap_or_else(|| fallback.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Value);

    impl DefaultProvider for Fixed {
        fn computed_default(&self, _def: &SettingDef, _instance: Option<&str>) -> Option<Value> {
            Some(self.0.clone())
        }
    }

    #[test]
    fn computed_defaults_fall_back_when_missing_or_invalid() {
        let fixed = SettingDef::int("a.b", Panel::Power, (0, 10, 1), 3);
        let computed = fixed.clone().computed();
        assert_eq!(
            default_of(&fixed, None, &Fixed(Value::Int(7))),
            Value::Int(3)
        );
        assert_eq!(
            default_of(&computed, None, &Fixed(Value::Int(7))),
            Value::Int(7)
        );
        assert_eq!(
            default_of(&computed, None, &Fixed(Value::Int(70))),
            Value::Int(3)
        );
        assert_eq!(
            default_of(&computed, None, &Fixed(Value::Bool(true))),
            Value::Int(3)
        );
        assert_eq!(default_of(&computed, None, &NoDefaults), Value::Int(3));
    }
}
