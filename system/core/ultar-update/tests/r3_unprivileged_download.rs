//! r3 finding F3 (major): `download` is open to every bus caller and (a) lets the caller
//! override the administrator's metered-network policy with its own flag, (b) throws away a
//! bundle the administrator staged from a stick and replaces it with whatever the (untrusted)
//! server offers. `apply` names no release, so the administrator then installs something
//! else than what was staged and shown – here an older release replayed by the server.
//!
//! Expected: (a) on a metered network a caller that is not an administrator cannot download
//! unless the policy allows metered use; (b) a staged bundle is replaced only by an
//! administrator (or `apply` carries the version/digest the caller saw and refuses others).

mod common;

use common::rig::*;
use common::*;
use ultar_update::bootctl::Slot;
use ultar_update::error::UpdateError;
use ultar_update::service::Job;

#[test]
fn caller_without_rights_cannot_override_the_metered_policy() {
    let rig = rig();
    rig.publish(&release((0, 7, 1), 2));
    rig.core.network(NET, true).unwrap();
    assert!(!rig.core.policy().allow_metered, "policy: no metered use");
    let job = Job::Download {
        allow_metered: true,
    };
    let result = rig.job(USER, job);
    assert!(
        matches!(
            result,
            Err(UpdateError::Metered | UpdateError::PermissionDenied)
        ),
        "an unprivileged caller downloaded on a metered network: {result:?}"
    );
}

#[test]
fn caller_without_rights_cannot_swap_what_the_administrator_staged() {
    let rig = rig();
    let chosen = release((0, 7, 2), 1);
    let path = rig.device.write_bundle("chosen.uup", &chosen);
    let offline = Job::Offline {
        path,
        allow_downgrade: false,
    };
    rig.job(ADMIN, offline).unwrap();
    let staged = rig.core.status().unwrap().progress.version;
    assert_eq!(staged.as_deref(), Some("0.7.2"));

    // The server still offers (or replays) the older 0.7.1; any caller may say "download".
    let replayed = release((0, 7, 1), 1);
    rig.publish(&replayed);
    rig.core.network(NET, false).unwrap();
    let download = Job::Download {
        allow_metered: false,
    };
    let _ = rig.job(USER, download);
    let staged = rig.core.status().unwrap().progress.version;
    assert_eq!(staged.as_deref(), Some("0.7.2"), "staging was replaced");

    rig.job(ADMIN, Job::Apply).unwrap();
    assert!(
        rig.device.images(Slot::B) == chosen.payloads,
        "the administrator applied 0.7.2 and got another release"
    );
}
