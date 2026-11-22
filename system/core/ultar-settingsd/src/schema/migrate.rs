//! Conversions of stored values between schema versions.

use super::key::KeyTemplate;
use crate::value::Value;
use std::collections::BTreeMap;

/// One change to the stored keys.
#[derive(Clone, Copy, Debug)]
pub enum MigrationStep {
    /// A key got a new name; `<id>` segments carry over.
    Rename {
        /// Old key template.
        from: &'static str,
        /// New key template.
        to: &'static str,
    },
    /// A key was dropped.
    Remove {
        /// Old key template.
        key: &'static str,
    },
    /// The value of a key changed its meaning or type; `None` drops it.
    Convert {
        /// Key template.
        key: &'static str,
        /// Conversion of the old value.
        convert: fn(&Value) -> Option<Value>,
    },
}

/// All steps that lead to one schema version.
#[derive(Clone, Debug)]
pub struct Migration {
    /// The version the stored values have after the steps.
    pub to_version: u32,
    /// The steps, applied in order.
    pub steps: Vec<MigrationStep>,
}

fn matching_keys(
    values: &BTreeMap<String, Value>,
    template: &'static str,
) -> Vec<(String, Option<String>)> {
    let Some(template) = KeyTemplate::parse(template) else {
        return Vec::new();
    };
    values
        .keys()
        .filter_map(|key| {
            template
                .matches(key)
                .map(|instance| (key.clone(), instance.map(str::to_owned)))
        })
        .collect()
}

impl MigrationStep {
    /// Applies the step to stored values; returns how many keys changed.
    pub fn apply(&self, values: &mut BTreeMap<String, Value>) -> usize {
        match *self {
            MigrationStep::Rename { from, to } => {
                let Some(target) = KeyTemplate::parse(to) else {
                    return 0;
                };
                let hits = matching_keys(values, from);
                for (key, instance) in &hits {
                    if let Some(value) = values.remove(key) {
                        values
                            .entry(target.instantiate(instance.as_deref()))
                            .or_insert(value);
                    }
                }
                hits.len()
            }
            MigrationStep::Remove { key } => {
                let hits = matching_keys(values, key);
                for (key, _) in &hits {
                    values.remove(key);
                }
                hits.len()
            }
            MigrationStep::Convert { key, convert } => {
                let hits = matching_keys(values, key);
                for (key, _) in &hits {
                    match values.get(key).and_then(convert) {
                        Some(value) => {
                            values.insert(key.clone(), value);
                        }
                        None => {
                            values.remove(key);
                        }
                    }
                }
                hits.len()
            }
        }
    }
}

/// Brings values stored under `from_version` up to `to_version`.
/// Returns the number of changed keys.
pub fn migrate(
    migrations: &[Migration],
    from_version: u32,
    to_version: u32,
    values: &mut BTreeMap<String, Value>,
) -> usize {
    let mut changed = 0;
    for migration in migrations {
        if migration.to_version > from_version && migration.to_version <= to_version {
            for step in &migration.steps {
                changed += step.apply(values);
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("look.theme".to_owned(), Value::text("noir")),
            ("display.a.zoom".to_owned(), Value::Float(1.5)),
            ("display.b.zoom".to_owned(), Value::Float(2.0)),
            ("old.flag".to_owned(), Value::Bool(true)),
        ])
    }

    fn zoom_to_percent(value: &Value) -> Option<Value> {
        match value {
            Value::Float(factor) if *factor < 2.0 => Some(Value::Int((factor * 100.0) as i64)),
            _ => None,
        }
    }

    fn history() -> Vec<Migration> {
        vec![
            Migration {
                to_version: 2,
                steps: vec![
                    MigrationStep::Rename {
                        from: "look.theme",
                        to: "appearance.theme",
                    },
                    MigrationStep::Remove { key: "old.flag" },
                ],
            },
            Migration {
                to_version: 3,
                steps: vec![
                    MigrationStep::Convert {
                        key: "display.<id>.zoom",
                        convert: zoom_to_percent,
                    },
                    MigrationStep::Rename {
                        from: "display.<id>.zoom",
                        to: "display.<id>.scale",
                    },
                ],
            },
        ]
    }

    #[test]
    fn all_steps_from_the_oldest_version() {
        let mut values = stored();
        let changed = migrate(&history(), 1, 3, &mut values);
        assert_eq!(changed, 5);
        assert_eq!(
            values,
            BTreeMap::from([
                ("appearance.theme".to_owned(), Value::text("noir")),
                ("display.a.scale".to_owned(), Value::Int(150)),
            ])
        );
    }

    #[test]
    fn only_the_missing_steps_run() {
        let mut values = stored();
        assert_eq!(migrate(&history(), 2, 3, &mut values), 3);
        assert!(values.contains_key("look.theme"));
        let mut values = stored();
        assert_eq!(migrate(&history(), 3, 3, &mut values), 0);
        assert_eq!(values, stored());
        let mut values = stored();
        assert_eq!(migrate(&history(), 1, 2, &mut values), 2);
        assert!(values.contains_key("display.a.zoom"));
    }

    #[test]
    fn rename_keeps_a_value_already_stored_under_the_new_name() {
        let mut values = stored();
        values.insert("appearance.theme".to_owned(), Value::text("light"));
        migrate(&history(), 1, 2, &mut values);
        assert_eq!(values.get("appearance.theme"), Some(&Value::text("light")));
        assert!(!values.contains_key("look.theme"));
    }

    #[test]
    fn malformed_templates_change_nothing() {
        let mut values = stored();
        let bad = MigrationStep::Rename {
            from: "look.theme",
            to: "<id>",
        };
        assert_eq!(bad.apply(&mut values), 0);
        let bad = MigrationStep::Remove { key: "a..b" };
        assert_eq!(bad.apply(&mut values), 0);
        assert_eq!(values, stored());
    }
}
