//! r3 finding F6 (minor): the stored rotation chain is never shortened. Links at or below the
//! burned-in serial are skipped when the chain is verified, but they stay in the file and
//! count against `MAX_CHAIN`. The 65th rotation in the life of a device is refused with
//! `RotationRefused` – also after a new system image has burned in the current key set –
//! and with it every bundle that carries that rotation. (`keys.rs` says "a system image with
//! newer burned-in keys resets the need"; the code does not do that.)
//!
//! Expected: links covered by the burned-in serial are dropped; a device whose image carries
//! the set of serial N accepts rotation N + 1.

mod common;

use common::*;
use ultar_update::keys::{KeySet, Rotation, TrustStore, MAX_CHAIN};
use ultar_update::sign::public_of;

fn rotation(serial: u64, secrets: &[[u8; 32]]) -> Vec<u8> {
    let mut keys: Vec<[u8; 32]> = secrets.iter().map(public_of).collect();
    keys.sort_unstable();
    let set = KeySet { serial, keys };
    Rotation { set, created: NOW }.sign(&PUBLISHER).unwrap()
}

#[test]
fn new_system_image_makes_room_for_further_rotations() {
    let device = Device::new();
    let (keys, state) = (&device.config.keys_dir, &device.config.state_dir);
    let mut trust = TrustStore::open(keys, state).unwrap();
    for serial in 1..=MAX_CHAIN as u64 {
        assert!(trust.accept(&rotation(serial, &[PUBLISHER])).unwrap());
    }
    drop(trust);

    // A new system image burns in the key set of serial 64.
    std::fs::write(keys.join("serial"), format!("{MAX_CHAIN}\n")).unwrap();
    let mut trust = TrustStore::open(keys, state).unwrap();
    assert_eq!(trust.serial(), MAX_CHAIN as u64);
    let next = trust.accept(&rotation(MAX_CHAIN as u64 + 1, &[PUBLISHER, STRANGER]));
    assert!(matches!(next, Ok(true)), "rotation 65: {next:?}");
}
