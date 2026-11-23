//! r3 finding F5 (minor, design): the time floor is raised from `created` of every manifest
//! whose signature verifies – before the manifest is judged for this device and without any
//! bound. One signed manifest with a wrong `created` (milliseconds instead of seconds, a
//! typo; the signing tool accepts any number) that a device merely *looks at* makes that
//! device refuse every normal release as "expired" for ever. Nothing on the device can undo
//! it, and it also hits devices the manifest was never meant for.
//!
//! Expected: a manifest this device refuses (other device, edition, channel …) does not move
//! its time floor; `uupdate-sign` refuses absurd `created`/`expires` values.

mod common;

use common::*;
use ultar_update::error::UpdateError;

#[test]
fn manifest_for_another_device_cannot_expire_all_future_releases() {
    let device = Device::new();
    let mut engine = device.engine();
    let mistake = release_with((0, 7, 1), 1, &PUBLISHER, |m| {
        m.devices = vec!["some-other-board".into()];
        m.created = NOW * 1000; // milliseconds by mistake
        m.expires = NOW * 1000 + 30 * 86_400_000;
    });
    let refused = engine.check(&mut MemSource::new(&mistake.bundle), "stable");
    assert!(matches!(refused, Err(UpdateError::WrongDevice)));

    let normal = release((0, 7, 2), 1);
    let offer = engine.check(&mut MemSource::new(&normal.bundle), "stable");
    assert!(
        offer.is_ok(),
        "a correct release is refused after a foreign manifest was seen: {:?}",
        offer.err()
    );
}
