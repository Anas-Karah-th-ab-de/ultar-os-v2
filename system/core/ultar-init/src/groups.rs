//! Memory groups (`cgroup2` on Linux, the group tree of the Ultar kernel;
//! `docs/integration/kernel-memory-groups.md`).
//!
//! Every service gets `<root>/<slice>/<name>` with the limits of its
//! service file; the launcher moves itself there before it executes the
//! service, so nothing the service starts escapes the group.
//!
//! Detected, not assumed: a tree without `cgroup.procs` at its root is
//! "not there" (development machine, a kernel without groups) and services
//! run without groups – reported once. Where the tree is there, a limit
//! the service file names must be applied: a missing `memory.*` or
//! `pids.*` file or a refused write refuses the start (fail closed). Only a
//! controller the kernel does not have at all (`cpu.*`, `io.*` on the
//! Ultar kernel) is skipped and reported.

use crate::spec::Slice;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Controller prefixes a service file may set limits for.
const ALLOWED_PREFIXES: [&str; 4] = ["memory.", "pids.", "cpu.", "io."];
/// Prefixes whose files must exist when the tree is there.
const REQUIRED_PREFIXES: [&str; 2] = ["memory.", "pids."];
/// Longest limit value.
const MAX_VALUE: usize = 64;

/// Why a group could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupError {
    /// A limit name that is not a controller file a service may set.
    Key(String),
    /// A limit value with characters a limit never has.
    Value(String),
    /// The kernel has no file for a limit that must be applied.
    Unsupported(String),
    /// Creating the group or writing a file failed.
    Io(String, String),
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key(key) => write!(f, "limit {key} not allowed"),
            Self::Value(key) => write!(f, "limit {key}: invalid value"),
            Self::Unsupported(key) => write!(f, "limit {key} not supported by the kernel"),
            Self::Io(what, error) => write!(f, "{what}: {error}"),
        }
    }
}

/// A prepared group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// Its directory.
    pub dir: PathBuf,
    /// Limits the kernel has no controller for (skipped).
    pub skipped: Vec<String>,
}

/// The group tree, if the kernel has one.
#[derive(Debug, Clone)]
pub struct GroupTree {
    root: Option<PathBuf>,
}

fn key_allowed(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_')
        && ALLOWED_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

fn value_allowed(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VALUE
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'+' | b'-'))
}

fn io_error(what: &Path, error: &std::io::Error) -> GroupError {
    GroupError::Io(what.display().to_string(), error.to_string())
}

/// Writes `+memory +pids` where the kernel wants controllers enabled for
/// the children (Linux); the Ultar kernel has no such file (always on).
fn enable_controllers(dir: &Path) {
    let control = dir.join("cgroup.subtree_control");
    if control.exists() {
        // Each separately: one the kernel lacks must not hold back the other.
        for controller in ["+memory", "+pids", "+cpu"] {
            let _ = std::fs::write(&control, controller);
        }
    }
}

impl GroupTree {
    /// The tree at `root` (normally `/sys/fs/cgroup`), if it is one.
    pub fn detect(root: &Path) -> Self {
        let present = root.join("cgroup.procs").is_file();
        Self {
            root: present.then(|| root.to_owned()),
        }
    }

    /// No tree: services run without groups.
    pub fn none() -> Self {
        Self { root: None }
    }

    /// Whether the kernel has a group tree.
    pub fn is_present(&self) -> bool {
        self.root.is_some()
    }

    /// Creates the group of service `name` and writes its limits. `None`
    /// without a tree.
    pub fn prepare(
        &self,
        slice: Slice,
        name: &str,
        limits: &BTreeMap<String, String>,
    ) -> Result<Option<Prepared>, GroupError> {
        for (key, value) in limits {
            if !key_allowed(key) {
                return Err(GroupError::Key(key.clone()));
            }
            if !value_allowed(value) {
                return Err(GroupError::Value(key.clone()));
            }
        }
        let Some(root) = &self.root else {
            return Ok(None);
        };
        let parent = root.join(slice.dir());
        enable_controllers(root);
        if !parent.is_dir() {
            std::fs::create_dir(&parent).map_err(|error| io_error(&parent, &error))?;
        }
        enable_controllers(&parent);
        let dir = parent.join(name);
        if !dir.is_dir() {
            std::fs::create_dir(&dir).map_err(|error| io_error(&dir, &error))?;
        }
        let mut skipped = Vec::new();
        for (key, value) in limits {
            let file = dir.join(key);
            if !file.exists() {
                if REQUIRED_PREFIXES
                    .iter()
                    .any(|prefix| key.starts_with(prefix))
                {
                    return Err(GroupError::Unsupported(key.clone()));
                }
                skipped.push(key.clone());
                continue;
            }
            std::fs::write(&file, value).map_err(|error| io_error(&file, &error))?;
        }
        Ok(Some(Prepared { dir, skipped }))
    }
}

/// Ends every process of the group at `dir` (`cgroup.kill`, where the
/// kernel has it). `false` when it could not be done that way.
pub fn kill_all(dir: &Path) -> bool {
    std::fs::write(dir.join("cgroup.kill"), "1").is_ok()
}

/// Process ids in the group at `dir`.
pub fn members(dir: &Path) -> Vec<u32> {
    std::fs::read_to_string(dir.join("cgroup.procs"))
        .map(|text| {
            text.lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Removes the (empty) group at `dir`; a group with processes stays.
pub fn remove(dir: &Path) {
    let _ = std::fs::remove_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree like the kernel's: every directory has the control files.
    fn fake_tree(tag: &str, with_cpu: bool) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ultar-init-groups-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("system/logd");
        std::fs::create_dir_all(&dir).unwrap();
        for file in ["cgroup.procs", "memory.max", "pids.max"] {
            std::fs::write(root.join(file), "").unwrap();
            std::fs::write(dir.join(file), "").unwrap();
        }
        if with_cpu {
            std::fs::write(dir.join("cpu.weight"), "").unwrap();
        }
        root
    }

    fn limits(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn limits_are_written_and_a_missing_controller_is_skipped_or_refused() {
        let root = fake_tree("write", false);
        let tree = GroupTree::detect(&root);
        assert!(tree.is_present());
        let wanted = limits(&[
            ("memory.max", "48M"),
            ("pids.max", "160"),
            ("cpu.weight", "50"),
        ]);
        let prepared = tree
            .prepare(Slice::System, "logd", &wanted)
            .unwrap()
            .unwrap();
        assert_eq!(prepared.dir, root.join("system/logd"));
        assert_eq!(prepared.skipped, ["cpu.weight"]);
        let read = |file: &str| std::fs::read_to_string(prepared.dir.join(file)).unwrap();
        assert_eq!(
            (read("memory.max").as_str(), read("pids.max").as_str()),
            ("48M", "160")
        );
        // A group whose kernel lacks the memory file is refused.
        std::fs::remove_file(prepared.dir.join("memory.max")).unwrap();
        assert_eq!(
            tree.prepare(Slice::System, "logd", &wanted),
            Err(GroupError::Unsupported("memory.max".into()))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keys_and_values_outside_the_controllers_are_refused_even_without_a_tree() {
        let tree = GroupTree::none();
        for (key, value, error) in [
            ("cgroup.procs", "1", GroupError::Key("cgroup.procs".into())),
            (
                "../memory.max",
                "1",
                GroupError::Key("../memory.max".into()),
            ),
            ("memory.max", "1\n2", GroupError::Value("memory.max".into())),
            ("memory.max", "", GroupError::Value("memory.max".into())),
        ] {
            assert_eq!(
                tree.prepare(Slice::Apps, "x", &limits(&[(key, value)])),
                Err(error)
            );
        }
        assert_eq!(
            tree.prepare(Slice::Apps, "x", &limits(&[("pids.max", "8")])),
            Ok(None)
        );
        assert!(!GroupTree::detect(Path::new("/nonexistent")).is_present());
        assert!(!GroupError::Io("a".into(), "b".into())
            .to_string()
            .is_empty());
    }

    #[test]
    fn members_and_removal_follow_the_files() {
        let root = fake_tree("members", true);
        let dir = root.join("system/logd");
        std::fs::write(dir.join("cgroup.procs"), "12\n13\n").unwrap();
        assert_eq!(members(&dir), [12, 13]);
        assert!(kill_all(&dir));
        remove(&root.join("nonexistent"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
