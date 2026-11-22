//! The value of one setting and its conversions (bus contract, TOML, text).

use std::fmt;
use ultar_bus::messages::settings::SettingValue;

/// Value of one setting.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// On/off switch.
    Bool(bool),
    /// Whole number.
    Int(i64),
    /// Fractional number.
    Float(f64),
    /// Identifier or text.
    Str(String),
    /// Ordered list of identifiers or texts.
    List(Vec<String>),
}

impl Value {
    /// Short stable name of the value's type, for diagnostics.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::List(_) => "list",
        }
    }

    /// Text value from a string slice.
    pub fn text(text: &str) -> Value {
        Value::Str(text.to_owned())
    }

    /// List value from string slices.
    pub fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|item| (*item).to_owned()).collect())
    }

    /// The form used in the settings files.
    pub fn to_toml(&self) -> toml::Value {
        match self {
            Value::Bool(flag) => toml::Value::Boolean(*flag),
            Value::Int(number) => toml::Value::Integer(*number),
            Value::Float(number) => toml::Value::Float(*number),
            Value::Str(text) => toml::Value::String(text.clone()),
            Value::List(items) => toml::Value::Array(
                items
                    .iter()
                    .map(|item| toml::Value::String(item.clone()))
                    .collect(),
            ),
        }
    }

    /// Reads the form used in the settings files; `None` for TOML values a
    /// setting can never have (tables, dates, mixed arrays).
    pub fn from_toml(value: &toml::Value) -> Option<Value> {
        match value {
            toml::Value::Boolean(flag) => Some(Value::Bool(*flag)),
            toml::Value::Integer(number) => Some(Value::Int(*number)),
            toml::Value::Float(number) => Some(Value::Float(*number)),
            toml::Value::String(text) => Some(Value::Str(text.clone())),
            toml::Value::Array(items) => items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<String>>>()
                .map(Value::List),
            toml::Value::Datetime(_) | toml::Value::Table(_) => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(flag) => write!(f, "{flag}"),
            Value::Int(number) => write!(f, "{number}"),
            Value::Float(number) => write!(f, "{number}"),
            Value::Str(text) => f.write_str(text),
            Value::List(items) => f.write_str(&items.join(",")),
        }
    }
}

impl From<SettingValue> for Value {
    fn from(value: SettingValue) -> Value {
        match value {
            SettingValue::Flag(flag) => Value::Bool(flag),
            SettingValue::Integer(number) => Value::Int(number),
            SettingValue::Decimal(number) => Value::Float(number),
            SettingValue::Text(text) => Value::Str(text),
            SettingValue::List(items) => Value::List(items),
        }
    }
}

impl From<Value> for SettingValue {
    fn from(value: Value) -> SettingValue {
        match value {
            Value::Bool(flag) => SettingValue::Flag(flag),
            Value::Int(number) => SettingValue::Integer(number),
            Value::Float(number) => SettingValue::Decimal(number),
            Value::Str(text) => SettingValue::Text(text),
            Value::List(items) => SettingValue::List(items),
        }
    }
}

impl From<&Value> for SettingValue {
    fn from(value: &Value) -> SettingValue {
        value.clone().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Value> {
        vec![
            Value::Bool(true),
            Value::Int(-7),
            Value::Float(1.25),
            Value::text("ar"),
            Value::list(&["a", "b"]),
            Value::List(Vec::new()),
        ]
    }

    #[test]
    fn toml_round_trip() {
        for value in samples() {
            assert_eq!(Value::from_toml(&value.to_toml()), Some(value));
        }
    }

    #[test]
    fn toml_rejects_shapes_no_setting_has() {
        let table = toml::Value::Table(toml::Table::new());
        assert_eq!(Value::from_toml(&table), None);
        let mixed = toml::Value::Array(vec![toml::Value::Integer(1)]);
        assert_eq!(Value::from_toml(&mixed), None);
    }

    #[test]
    fn bus_round_trip() {
        for value in samples() {
            let wire: SettingValue = (&value).into();
            assert_eq!(Value::from(wire), value);
        }
    }

    #[test]
    fn display_and_type_names() {
        let shown: Vec<String> = samples().iter().map(ToString::to_string).collect();
        assert_eq!(shown, ["true", "-7", "1.25", "ar", "a,b", ""]);
        let names: Vec<&str> = samples().iter().map(Value::type_name).collect();
        assert_eq!(names, ["bool", "int", "float", "string", "list", "list"]);
    }
}
