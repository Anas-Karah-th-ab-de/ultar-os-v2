//! The model size the person chose (`assistant.model`): the largest installed model of that
//! class answers, and the state on the bus reports it. A class with no installed model leaves
//! the model service's own pick (the state still tells the truth about what is there).

use ultar_bus::messages::llm::ModelClass;
use ultar_bus::messages::settings::SettingValue;

/// The setting that names the model size.
pub const MODEL_KEY: &str = "assistant.model";

/// The class a value of the setting names; `None` for a value of the wrong type (it changes
/// nothing). An unknown word falls back to the schema's default, small.
pub fn of(value: &SettingValue) -> Option<ModelClass> {
    match value {
        SettingValue::Text(t) => Some(match t.as_str() {
            "medium" => ModelClass::Medium,
            "large" => ModelClass::Large,
            _ => ModelClass::Small,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_of_the_schema_become_classes() {
        assert_eq!(
            of(&SettingValue::Text("small".into())),
            Some(ModelClass::Small)
        );
        assert_eq!(
            of(&SettingValue::Text("medium".into())),
            Some(ModelClass::Medium)
        );
        assert_eq!(
            of(&SettingValue::Text("large".into())),
            Some(ModelClass::Large)
        );
        // An unknown word falls back to the default; a wrong type changes nothing.
        assert_eq!(
            of(&SettingValue::Text("huge".into())),
            Some(ModelClass::Small)
        );
        assert_eq!(of(&SettingValue::Flag(true)), None);
        assert_eq!(of(&SettingValue::Integer(2)), None);
    }
}
