//! Loading one settings file: read, verify, migrate, check against the schema.

use super::disk::{quarantine, read_file, ReadOutcome};
use super::format::{decode, FileContent};
use super::{FileId, Notice, MAX_NOTICES};
use crate::schema::{migrate, InstanceKind, Registry, Scope};
use crate::value::Value;
use std::collections::BTreeMap;
use std::path::Path;
use ultar_bus::messages::settings::StoreNoticeKind;

/// Collects notices up to the limit.
pub(super) struct Notices<'a> {
    pub(super) file: &'a str,
    pub(super) list: &'a mut Vec<Notice>,
}

impl Notices<'_> {
    pub(super) fn push(&mut self, kind: StoreNoticeKind, detail: &str) {
        if self.list.len() < MAX_NOTICES {
            self.list.push(Notice {
                file: self.file.to_owned(),
                kind,
                detail: detail.to_owned(),
            });
        }
    }
}

/// Result of loading: the usable content and whether the file on disk
/// differs from it and must be rewritten.
pub(super) struct Loaded {
    pub(super) content: FileContent,
    pub(super) rewrite: bool,
}

fn fresh(version: u32) -> FileContent {
    FileContent {
        schema: version,
        ..FileContent::default()
    }
}

fn put_aside(dir: &Path, notices: &mut Notices<'_>, kind: StoreNoticeKind) {
    match quarantine(dir, notices.file) {
        Ok(copy) => notices.push(kind, &copy),
        // No copy could be made: say so instead of claiming one exists.
        Err(_) => notices.push(StoreNoticeKind::Unreadable, ""),
    }
}

/// Drops entries the schema does not accept for this place.
fn check_values(
    values: &mut BTreeMap<String, Value>,
    registry: &Registry,
    scope: Scope,
    displays: Option<&[String]>,
    notices: &mut Notices<'_>,
) -> bool {
    let mut dropped = false;
    let keys: Vec<String> = values.keys().cloned().collect();
    for key in keys {
        let verdict = match registry.resolve(&key) {
            Ok(found) if found.def.scope == scope => {
                let instance_known = match (found.def.instance, displays) {
                    (InstanceKind::Display, Some(displays)) => found
                        .instance
                        .as_ref()
                        .is_some_and(|instance| displays.contains(instance)),
                    _ => true,
                };
                if !instance_known {
                    Some(StoreNoticeKind::UnknownKeyDropped)
                } else {
                    let stored = values.remove(&key);
                    match stored.map(|value| found.def.value_type.accept(value)) {
                        Some(Ok(value)) => {
                            values.insert(key.clone(), value);
                            None
                        }
                        _ => Some(StoreNoticeKind::InvalidValueDropped),
                    }
                }
            }
            _ => Some(StoreNoticeKind::UnknownKeyDropped),
        };
        if let Some(kind) = verdict {
            values.remove(&key);
            notices.push(kind, &key);
            dropped = true;
        }
    }
    dropped
}

fn check_content(
    content: &mut FileContent,
    id: FileId,
    registry: &Registry,
    notices: &mut Notices<'_>,
) -> bool {
    let mut dropped = false;
    match id {
        FileId::System | FileId::User => {
            let scope = if id == FileId::System {
                Scope::System
            } else {
                Scope::User
            };
            dropped |= check_values(&mut content.values, registry, scope, None, notices);
            if !content.combos.is_empty() {
                content.combos.clear();
                notices.push(StoreNoticeKind::UnknownKeyDropped, "combo");
                dropped = true;
            }
        }
        FileId::Displays => {
            if !content.values.is_empty() {
                content.values.clear();
                notices.push(StoreNoticeKind::UnknownKeyDropped, "values");
                dropped = true;
            }
            for combo in content.combos.values_mut() {
                let displays = combo.displays.clone();
                dropped |= check_values(
                    &mut combo.values,
                    registry,
                    Scope::DisplayCombination,
                    Some(&displays),
                    notices,
                );
            }
        }
        FileId::Journal => {}
    }
    dropped
}

fn migrate_content(content: &mut FileContent, registry: &Registry) -> usize {
    let (from, to) = (content.schema, registry.version());
    let mut changed = migrate(registry.migrations(), from, to, &mut content.values);
    for combo in content.combos.values_mut() {
        changed += migrate(registry.migrations(), from, to, &mut combo.values);
    }
    content.schema = to;
    changed
}

/// Loads `dir/<file>`; never fails: every problem ends in defaults plus a
/// notice, with a quarantine copy of anything that could not be used.
pub(super) fn load_file(
    dir: &Path,
    id: FileId,
    registry: &Registry,
    notices: &mut Notices<'_>,
) -> Loaded {
    let version = registry.version();
    let unusable = |rewrite| Loaded {
        content: fresh(version),
        rewrite,
    };
    let text = match read_file(dir, notices.file) {
        Ok(ReadOutcome::Missing) => return unusable(false),
        Ok(ReadOutcome::Text(text)) => text,
        Ok(ReadOutcome::Damaged) => {
            put_aside(dir, notices, StoreNoticeKind::Quarantined);
            return unusable(true);
        }
        Err(_) => {
            notices.push(StoreNoticeKind::Unreadable, "");
            return unusable(false);
        }
    };
    let mut content = match decode(&text) {
        Ok(content) => content,
        Err(_) => {
            put_aside(dir, notices, StoreNoticeKind::Quarantined);
            return unusable(true);
        }
    };
    if content.schema > version {
        put_aside(dir, notices, StoreNoticeKind::NewerSchema);
        return unusable(true);
    }
    let mut rewrite = false;
    if content.schema < version {
        let from = content.schema;
        migrate_content(&mut content, registry);
        notices.push(StoreNoticeKind::Migrated, &format!("{from}->{version}"));
        rewrite = true;
    }
    rewrite |= check_content(&mut content, id, registry, notices);
    Loaded { content, rewrite }
}
