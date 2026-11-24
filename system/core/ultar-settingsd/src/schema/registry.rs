//! The registry: every setting is declared exactly once, here.

use super::area::Panel;
use super::def::{InstanceKind, Scope, SettingDef};
use super::key::{is_valid_key, is_valid_segment, KeyTemplate};
use super::migrate::Migration;
use super::types::{ValueError, ValueType};
use std::collections::BTreeMap;
use std::fmt;

/// A declaration that cannot be registered.
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaError {
    /// The key template is not well formed.
    InvalidKey(&'static str),
    /// Two declarations cover the same keys.
    DuplicateKey(&'static str),
    /// The default is not a valid value of the declared type.
    InvalidDefault(&'static str, ValueError),
    /// Range with `min > max`, a step below 1 or a non-finite bound.
    InvalidRange(&'static str),
    /// No choices, a repeated choice or a choice that is not an identifier.
    InvalidChoices(&'static str),
    /// `<id>` in the key and the declared instance kind disagree, or the
    /// scope does not fit the instance kind.
    InstanceMismatch(&'static str),
    /// A confirm time of zero seconds.
    InvalidRisk(&'static str),
    /// Migrations not in ascending order or beyond the schema version.
    InvalidMigration(u32),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::InvalidKey(key) => write!(f, "invalid setting key {key}"),
            SchemaError::DuplicateKey(key) => write!(f, "setting {key} declared twice"),
            SchemaError::InvalidDefault(key, error) => {
                write!(f, "default of {key} is invalid: {error}")
            }
            SchemaError::InvalidRange(key) => write!(f, "range of {key} is invalid"),
            SchemaError::InvalidChoices(key) => write!(f, "choices of {key} are invalid"),
            SchemaError::InstanceMismatch(key) => {
                write!(f, "instance kind or scope of {key} does not fit its key")
            }
            SchemaError::InvalidRisk(key) => write!(f, "confirm time of {key} is zero"),
            SchemaError::InvalidMigration(version) => {
                write!(f, "migration to version {version} is out of order")
            }
        }
    }
}

impl std::error::Error for SchemaError {}

/// Why a key does not name a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// The key is not syntactically valid.
    InvalidKey,
    /// No setting is declared under the key.
    UnknownKey,
}

/// A concrete key matched to its declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved<'r> {
    /// The declaration.
    pub def: &'r SettingDef,
    /// The concrete key.
    pub key: String,
    /// The `<id>` segment of the key, if the declaration has one.
    pub instance: Option<String>,
}

/// Collects declarations and checks them.
#[derive(Debug, Default)]
pub struct RegistryBuilder {
    version: u32,
    defs: Vec<SettingDef>,
    migrations: Vec<Migration>,
}

fn check_type(def: &SettingDef) -> Result<(), SchemaError> {
    match def.value_type {
        ValueType::Int { min, max, step } if min > max || step < 1 => {
            Err(SchemaError::InvalidRange(def.key))
        }
        ValueType::Float { min, max, step }
            if !(min.is_finite() && max.is_finite() && step.is_finite())
                || min > max
                || step < 0.0 =>
        {
            Err(SchemaError::InvalidRange(def.key))
        }
        ValueType::Enum(choices) => {
            let distinct = choices
                .iter()
                .enumerate()
                .all(|(index, choice)| !choices[..index].contains(choice));
            if choices.is_empty() || !distinct || !choices.iter().all(|c| is_valid_segment(c)) {
                Err(SchemaError::InvalidChoices(def.key))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn check_def(def: &SettingDef) -> Result<KeyTemplate, SchemaError> {
    let template = KeyTemplate::parse(def.key).ok_or(SchemaError::InvalidKey(def.key))?;
    let instance_fits = match def.instance {
        InstanceKind::None => {
            !template.has_instance()
                && (def.scope != Scope::DisplayCombination || def.panel == Panel::Display)
        }
        InstanceKind::Display => template.has_instance() && def.scope == Scope::DisplayCombination,
        InstanceKind::App => template.has_instance() && def.scope != Scope::DisplayCombination,
    };
    if !instance_fits {
        return Err(SchemaError::InstanceMismatch(def.key));
    }
    if matches!(def.risk, super::def::Risk::ConfirmRevert { seconds: 0 }) {
        return Err(SchemaError::InvalidRisk(def.key));
    }
    check_type(def)?;
    def.value_type
        .validate(def.default.base())
        .map_err(|error| SchemaError::InvalidDefault(def.key, error))?;
    Ok(template)
}

impl RegistryBuilder {
    /// Sets the schema version the store files are written with.
    pub fn version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Adds one declaration.
    pub fn setting(mut self, def: SettingDef) -> Self {
        self.defs.push(def);
        self
    }

    /// Adds declarations.
    pub fn settings(mut self, defs: impl IntoIterator<Item = SettingDef>) -> Self {
        self.defs.extend(defs);
        self
    }

    /// Adds the steps that lead to one schema version.
    pub fn migration(mut self, migration: Migration) -> Self {
        self.migrations.push(migration);
        self
    }

    /// Checks everything and builds the registry.
    pub fn build(self) -> Result<Registry, SchemaError> {
        let mut exact = BTreeMap::new();
        let mut templates: Vec<(KeyTemplate, usize)> = Vec::new();
        for (index, def) in self.defs.iter().enumerate() {
            let template = check_def(def)?;
            let duplicate = if template.has_instance() {
                let probe = template.instantiate(Some("x"));
                let clash = templates
                    .iter()
                    .any(|(other, _)| other.matches(&probe).is_some());
                templates.push((template, index));
                clash
            } else {
                exact.insert(def.key, index).is_some()
            };
            if duplicate {
                return Err(SchemaError::DuplicateKey(def.key));
            }
        }
        let mut last = 1;
        for migration in &self.migrations {
            if migration.to_version <= last || migration.to_version > self.version.max(1) {
                return Err(SchemaError::InvalidMigration(migration.to_version));
            }
            last = migration.to_version;
        }
        Ok(Registry {
            version: self.version.max(1),
            defs: self.defs,
            exact,
            templates,
            migrations: self.migrations,
        })
    }
}

/// All declared settings.
#[derive(Debug)]
pub struct Registry {
    version: u32,
    defs: Vec<SettingDef>,
    exact: BTreeMap<&'static str, usize>,
    templates: Vec<(KeyTemplate, usize)>,
    migrations: Vec<Migration>,
}

impl Registry {
    /// Starts an empty registry of schema version 1.
    pub fn builder() -> RegistryBuilder {
        RegistryBuilder {
            version: 1,
            ..RegistryBuilder::default()
        }
    }

    /// Schema version written into the store files.
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Every declaration in declaration order.
    pub fn defs(&self) -> &[SettingDef] {
        &self.defs
    }

    /// Declarations of one panel in declaration order.
    pub fn panel_defs(&self, panel: Panel) -> impl Iterator<Item = &SettingDef> {
        self.defs.iter().filter(move |def| def.panel == panel)
    }

    /// The migrations in ascending order.
    pub fn migrations(&self) -> &[Migration] {
        &self.migrations
    }

    /// Finds the declaration with exactly this key template.
    pub fn def(&self, template: &str) -> Option<&SettingDef> {
        self.defs.iter().find(|def| def.key == template)
    }

    /// Matches a concrete key to its declaration. Keys declared without
    /// instance win over templates.
    pub fn resolve(&self, key: &str) -> Result<Resolved<'_>, LookupError> {
        if !is_valid_key(key) {
            return Err(LookupError::InvalidKey);
        }
        if let Some(def) = self.exact.get(key).and_then(|index| self.defs.get(*index)) {
            return Ok(Resolved {
                def,
                key: key.to_owned(),
                instance: None,
            });
        }
        self.templates
            .iter()
            .find_map(|(template, index)| {
                let instance = template.matches(key)??;
                Some(Resolved {
                    def: self.defs.get(*index)?,
                    key: key.to_owned(),
                    instance: Some(instance.to_owned()),
                })
            })
            .ok_or(LookupError::UnknownKey)
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
