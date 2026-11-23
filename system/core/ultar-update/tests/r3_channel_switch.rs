//! r3 finding F4 (minor): the channel is checked when a bundle is fetched, but not when it is
//! installed (`Engine::open_verified_stage` passes `None`), and `set_policy` keeps the staged
//! bundle. After the administrator has moved the device from `beta` to `stable`, the next
//! automatic run installs the staged beta release.
//!
//! Expected: a bundle staged from the network is installed only while its channel is the
//! channel of the policy; changing the channel drops it.

mod common;

use common::rig::*;
use common::*;
use ultar_update::contract::Phase;
use ultar_update::service::Job;

#[test]
fn staged_bundle_of_the_old_channel_is_not_installed_after_a_channel_change() {
    let rig = rig();
    let mut policy = rig.core.policy();
    policy.channel = "beta".into();
    policy.automatic = false;
    rig.core.set_policy(ADMIN, &policy).unwrap();
    let beta = release_with((0, 8, 0), 2, &PUBLISHER, |m| m.channel = "beta".into());
    rig.publish(&beta);
    rig.core.network(NET, false).unwrap();
    let download = Job::Download {
        allow_metered: false,
    };
    rig.job(USER, download).unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Staged);

    policy.channel = "stable".into();
    policy.automatic = true;
    rig.core.set_policy(ADMIN, &policy).unwrap();
    *rig.published.lock().unwrap() = None; // nothing on "stable"
    let _ = rig.core.tick();

    let progress = rig.core.status().unwrap().progress;
    assert!(
        rig.device.slot_b_untouched(),
        "a beta release was installed on the stable channel: {progress:?}"
    );
}
