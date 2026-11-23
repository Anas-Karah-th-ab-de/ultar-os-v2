//! r3 finding F8 (minor, hardening): `EngineConfig::state_dir` is documented as "root only",
//! and the rollback floor, the slot manifests, the key chain and the staging marker (with the
//! downgrade permission) are trusted without a signature because they live there. The code
//! creates the directories with the process umask (0755 with the usual 022; world-writable
//! with a lax umask) and never checks owner or mode of an existing directory.
//!
//! Expected: the state directory tree is created with mode 0700 (and an existing one that
//! others can write is refused).

mod common;

use common::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn state_directory_is_created_private() {
    let device = Device::new();
    let mut engine = device.engine();
    install(&mut engine, &release((0, 7, 1), 2)).unwrap();
    for name in ["", "state", "slots", "staging"] {
        let path = device.config.state_dir.join(name);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o077, 0, "{} has mode {mode:o}", path.display());
    }
}
