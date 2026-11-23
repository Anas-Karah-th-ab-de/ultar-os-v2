//! Security review r3 (device identity), finding R3-7 (minor): the private files next to the key
//! (`cloud.token`, `device.state`) are written through a temporary file with a fixed name that is
//! opened without `O_EXCL`/`O_NOFOLLOW`. Whoever can create `<file>.part` in the state directory
//! gets the secret written to a place of his choice, and the final file becomes his link.
//! The key file itself is safe (`keystore.rs` uses a random name and `create_new`).
//!
//! This test FAILS on the reviewed code.
#![cfg(unix)]

use std::os::unix::fs::symlink;
use ultar_cloud::http::write_atomic;

#[test]
fn r3_7_a_private_file_is_not_written_through_a_planted_link() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (dest, loot) = (dir.path().join("cloud.token"), outside.path().join("loot"));
    std::fs::write(&loot, "").unwrap();
    symlink(&loot, dir.path().join("cloud.token.part")).unwrap();

    let _ = write_atomic(&dest, b"account-token-secret", true);

    assert_eq!(
        std::fs::read_to_string(&loot).unwrap(),
        "",
        "the token was written to the link's target"
    );
    let is_link = std::fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink());
    assert!(
        !is_link,
        "the token file is now a link out of the state directory"
    );
}
