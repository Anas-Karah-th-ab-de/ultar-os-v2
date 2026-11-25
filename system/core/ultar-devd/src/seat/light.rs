//! The torch's control file (camera light, `ultar_hal_system::torch`) for the power
//! service: owner root, the configured group (the power service's, 819), mode `0660`. The
//! power service has no other kernel rights; this is the one sysfs file it may write.
//!
//! Like the seat nodes, fail closed: only a regular file below the root's `sys/` whose
//! owner is root is changed, mode first narrowed, then group, then the final mode.

use std::fs;
use std::os::unix::fs::{chown, MetadataExt, PermissionsExt};
use std::path::Path;

/// What happened to the torch's control file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Light {
    /// No torch on this device.
    None,
    /// Already as wanted.
    Unchanged,
    /// Group and mode set.
    Set,
    /// Not a root-owned regular file below `sys/`: left alone.
    Refused,
    /// The change failed.
    Failed(String),
}

/// Gives the torch's control file below `root` to `group` (mode `0660`).
pub fn apply_torch(root: &Path, group: u32) -> Light {
    let Some(node) = ultar_hal_system::torch::find(root) else {
        return Light::None;
    };
    let path = node.control();
    let Ok(real) = fs::canonicalize(&path) else {
        return Light::Refused;
    };
    let sys = fs::canonicalize(root.join("sys")).unwrap_or_else(|_| root.join("sys"));
    let meta = match fs::metadata(&real) {
        Ok(m) if m.is_file() && m.uid() == 0 && real.starts_with(&sys) => m,
        _ => return Light::Refused,
    };
    let mode = meta.mode() & 0o7777;
    if (meta.gid(), mode) == (group, 0o660) {
        return Light::Unchanged;
    }
    let set = |m: u32| fs::set_permissions(&real, fs::Permissions::from_mode(m));
    match set(mode & 0o660)
        .and_then(|()| chown(&real, Some(0), Some(group)))
        .and_then(|()| set(0o660))
    {
        Ok(()) => Light::Set,
        Err(e) => Light::Failed(e.to_string()),
    }
}

/// One line for the program's report.
pub fn report(light: &Light) -> Option<String> {
    match light {
        Light::None => None,
        Light::Unchanged => Some("torch unchanged".into()),
        Light::Set => Some("torch set".into()),
        Light::Refused => Some("torch refused".into()),
        Light::Failed(e) => Some(format!("torch failed ({e})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_torch_no_change() {
        let root = std::env::temp_dir().join(format!("light-none-{}", std::process::id()));
        assert_eq!(apply_torch(&root, 819), Light::None);
    }

    #[test]
    fn a_file_outside_sys_or_of_another_owner_is_left_alone() {
        let root = std::env::temp_dir().join(format!("light-own-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let p = root.join(ultar_hal_system::torch::SAMSUNG);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "0").unwrap();
        let me = fs::metadata(&p).unwrap().uid();
        let got = apply_torch(&root, 819);
        if me == 0 {
            // As root in a test container: the file is changed.
            assert!(matches!(got, Light::Set | Light::Failed(_)), "{got:?}");
        } else {
            assert_eq!(got, Light::Refused);
        }
        let _ = fs::remove_dir_all(root);
    }
}
