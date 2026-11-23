//! r3 finding F2 (major): a staged bundle that can no longer be installed (here: expired)
//! blocks automatic updates for ever. `Core::automatic` sees "staged", calls `apply`, gets
//! the refusal and returns – on every run. The newer release on the server is never fetched
//! until an administrator cancels by hand. The same happens when the staged bundle was signed
//! by a key that has been rotated out, or when the rollback floor moved above it.
//!
//! Expected: a staged bundle that verification refuses is dropped, and the automatic run
//! goes on with what the source offers.

mod common;

use common::rig::*;
use common::*;
use ultar_update::contract::Phase;
use ultar_update::error::UpdateError;
use ultar_update::service::Job;

const DAY: u64 = 86_400;

#[test]
fn expired_staged_bundle_does_not_block_automatic_updates() {
    let rig = rig();
    rig.publish(&release((0, 7, 1), 2));
    rig.core.network(NET, false).unwrap();
    let download = Job::Download {
        allow_metered: false,
    };
    rig.job(USER, download).unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Staged);

    // The device was off for three months. The staged release has expired; a newer one is out.
    rig.device.clock.set(NOW + 90 * DAY);
    let newer = release_with((0, 7, 2), 2, &PUBLISHER, |m| {
        m.created = NOW + 89 * DAY;
        m.expires = NOW + 120 * DAY;
    });
    rig.publish(&newer);

    let mut last = Ok(());
    for _ in 0..3 {
        last = rig.core.tick();
    }
    let progress = rig.core.status().unwrap().progress;
    assert!(
        !matches!(last, Err(UpdateError::Expired)),
        "every automatic run ends with 'expired': {progress:?}"
    );
    assert_eq!(progress.phase, Phase::RebootRequired, "{progress:?}");
    assert_eq!(progress.version.as_deref(), Some("0.7.2"));
}
