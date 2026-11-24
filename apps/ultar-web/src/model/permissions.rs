//! Site permissions: what each site may use (camera, microphone, location, notifications).
//! Nothing is allowed by default; a site asks, the user answers in the browser's permission
//! prompt and may let the answer be remembered. Answers of private tabs are never written.

use super::store::{self, StoreError};
use crate::engine::PermissionKind;
use std::collections::BTreeMap;
use std::path::Path;

/// Most remembered answers.
pub const MAX_DECISIONS: usize = 2000;

/// The state of one right for one site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Decision {
    /// Ask each time (default).
    #[default]
    Ask,
    /// Allowed.
    Allow,
    /// Blocked.
    Block,
}

impl Decision {
    /// Every decision, in the order of the page panel's choices.
    pub const ALL: [Decision; 3] = [Decision::Ask, Decision::Allow, Decision::Block];

    /// Catalog key of its name.
    pub fn key(self) -> &'static str {
        match self {
            Decision::Ask => "web.perm.ask",
            Decision::Allow => "web.perm.allow",
            Decision::Block => "web.perm.block",
        }
    }
}

/// Remembered answers per site.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SitePermissions {
    decisions: BTreeMap<(String, PermissionKind), Decision>,
}

impl SitePermissions {
    /// What `host` may do with `kind`.
    pub fn get(&self, host: &str, kind: PermissionKind) -> Decision {
        self.decisions
            .get(&(host.to_string(), kind))
            .copied()
            .unwrap_or_default()
    }

    /// Remembers an answer (`Ask` forgets it). Returns `false` when the list is full.
    pub fn set(&mut self, host: &str, kind: PermissionKind, decision: Decision) -> bool {
        let key = (host.to_string(), kind);
        if decision == Decision::Ask {
            self.decisions.remove(&key);
            return true;
        }
        if self.decisions.len() >= MAX_DECISIONS && !self.decisions.contains_key(&key) {
            return false;
        }
        self.decisions.insert(key, decision);
        true
    }

    /// Number of remembered answers.
    pub fn len(&self) -> usize {
        self.decisions.len()
    }

    /// `true` without remembered answers.
    pub fn is_empty(&self) -> bool {
        self.decisions.is_empty()
    }

    /// Reads the file; lines `host  kind-code  allow|block`.
    pub fn load(path: &Path) -> Result<SitePermissions, StoreError> {
        let mut p = SitePermissions::default();
        for (n, row) in store::read(path)?.into_iter().enumerate() {
            let [host, code, value] = row.as_slice() else {
                return Err(StoreError::Damaged(n + 1));
            };
            let kind = code
                .parse::<u8>()
                .ok()
                .and_then(PermissionKind::from_code)
                .ok_or(StoreError::Damaged(n + 1))?;
            let decision = match value.as_str() {
                "allow" => Decision::Allow,
                "block" => Decision::Block,
                _ => return Err(StoreError::Damaged(n + 1)),
            };
            if crate::url::host_of(&format!("https://{host}/")).as_deref() != Some(host.as_str()) {
                return Err(StoreError::Damaged(n + 1));
            }
            p.set(host, kind, decision);
        }
        Ok(p)
    }

    /// Writes the file atomically.
    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        let rows: Vec<Vec<String>> = self
            .decisions
            .iter()
            .map(|((host, kind), d)| {
                let value = if *d == Decision::Allow {
                    "allow"
                } else {
                    "block"
                };
                vec![host.clone(), kind.code().to_string(), value.to_string()]
            })
            .collect();
        store::write(path, &rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_ask_and_round_trip() {
        let mut p = SitePermissions::default();
        assert_eq!(p.get("a.example", PermissionKind::Camera), Decision::Ask);
        assert!(p.set("a.example", PermissionKind::Camera, Decision::Allow));
        assert!(p.set("b.example", PermissionKind::Location, Decision::Block));
        assert_eq!(p.len(), 2);
        let dir = std::env::temp_dir().join(format!("ultar-web-perm-{}", std::process::id()));
        let path = dir.join("p.tsv");
        p.save(&path).unwrap();
        assert_eq!(SitePermissions::load(&path).unwrap(), p);
        for bad in [
            "x\t9\tallow\n",
            "x\t1\tmaybe\n",
            "x\t1\n",
            "a b\t1\tallow\n",
        ] {
            std::fs::write(&path, bad).unwrap();
            assert!(SitePermissions::load(&path).is_err(), "{bad:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(p.set("a.example", PermissionKind::Camera, Decision::Ask));
        assert_eq!(p.get("a.example", PermissionKind::Camera), Decision::Ask);
        for i in 0..MAX_DECISIONS {
            p.set(&format!("h{i}"), PermissionKind::Camera, Decision::Block);
        }
        assert!(!p.set("full", PermissionKind::Camera, Decision::Allow));
        assert!(!p.is_empty());
        assert!(Decision::ALL
            .iter()
            .all(|d| d.key().starts_with("web.perm.")));
    }
}
