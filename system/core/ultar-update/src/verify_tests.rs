use super::*;
use crate::manifest::tests::sample;
use crate::sign::{public_of, seal};

const SECRET: [u8; 32] = [42; 32];

fn sign(m: &Manifest) -> Vec<u8> {
    seal(
        &MANIFEST_MAGIC,
        MANIFEST_DOMAIN,
        &m.encode().unwrap(),
        &SECRET,
    )
    .unwrap()
}

fn device() -> DeviceIdentity {
    DeviceIdentity {
        arch: "x86_64".into(),
        edition: "desktop".into(),
        device: "pc-uefi".into(),
    }
}

const ROLES: [Role; 2] = [Role::Kernel, Role::System];

fn facts(device: &DeviceIdentity) -> Admission<'_> {
    Admission {
        device,
        roles: &ROLES,
        floor: 3,
        current: Version::new(0, 7, 0),
        now: 1_796_000_000,
        tolerance: DEFAULT_EXPIRY_TOLERANCE,
        channel: Some("stable"),
        allow_downgrade: false,
    }
}

fn check(
    m: &Manifest,
    edit: impl FnOnce(&mut Admission<'_>),
) -> Result<VerifiedManifest, UpdateError> {
    let device = device();
    let mut f = facts(&device);
    edit(&mut f);
    admit(authenticate(&sign(m), &[public_of(&SECRET)])?, &f)
}

#[test]
fn a_good_manifest_is_admitted() {
    let m = sample();
    let v = check(&m, |_| {}).unwrap();
    assert_eq!(v.manifest(), &m);
    assert_eq!(v.signed(), sign(&m).as_slice());
    assert_eq!(v.digest(), <[u8; 32]>::from(Sha256::digest(sign(&m))));
}

#[test]
fn authentication_failures() {
    let signed = sign(&sample());
    assert!(matches!(
        authenticate(&signed, &[]),
        Err(UpdateError::NoTrustedKeys)
    ));
    assert!(matches!(
        authenticate(&signed, &[public_of(&[1; 32])]),
        Err(UpdateError::UnknownKey)
    ));
    let mut tampered = signed.clone();
    tampered[40] ^= 0x20;
    assert!(matches!(
        authenticate(&tampered, &[public_of(&SECRET)]),
        Err(UpdateError::BadSignature)
    ));
    let mut longer = signed.clone();
    longer.push(0);
    assert!(matches!(
        authenticate(&longer, &[public_of(&SECRET)]),
        Err(UpdateError::Malformed(_))
    ));
    // Correctly signed, but not a canonical manifest: refused after the signature check.
    let junk = seal(
        &MANIFEST_MAGIC,
        MANIFEST_DOMAIN,
        b"version=9.9.9\n",
        &SECRET,
    )
    .unwrap();
    assert!(matches!(
        authenticate(&junk, &[public_of(&SECRET)]),
        Err(UpdateError::Malformed(_))
    ));
    // Signed for another purpose (rotation domain) with the right key.
    let other = seal(
        &MANIFEST_MAGIC,
        crate::sign::KEYS_DOMAIN,
        &sample().encode().unwrap(),
        &SECRET,
    )
    .unwrap();
    assert!(matches!(
        authenticate(&other, &[public_of(&SECRET)]),
        Err(UpdateError::BadSignature)
    ));
}

#[test]
fn expiry_with_tolerance() {
    let m = sample();
    let edge = m.expires + DEFAULT_EXPIRY_TOLERANCE;
    assert!(check(&m, |f| f.now = edge).is_ok());
    assert!(matches!(
        check(&m, |f| f.now = edge + 1),
        Err(UpdateError::Expired)
    ));
    assert!(matches!(
        check(&m, |f| {
            f.now = m.expires + 1;
            f.tolerance = 0;
        }),
        Err(UpdateError::Expired)
    ));
    // A clock that is far behind does not block an update.
    assert!(check(&m, |f| f.now = 0).is_ok());
    let mut forever = sample();
    forever.expires = u64::MAX;
    assert!(check(&forever, |f| f.now = u64::MAX).is_ok());
}

#[test]
fn wrong_target() {
    let m = sample();
    let mut d = device();
    d.arch = "aarch64".into();
    let auth = authenticate(&sign(&m), &[public_of(&SECRET)]).unwrap();
    assert!(matches!(
        admit(auth.clone(), &facts(&d)),
        Err(UpdateError::WrongArch)
    ));
    let mut d = device();
    d.edition = "phone".into();
    assert!(matches!(
        admit(auth.clone(), &facts(&d)),
        Err(UpdateError::WrongEdition)
    ));
    let mut d = device();
    d.device = "a30".into();
    assert!(matches!(
        admit(auth, &facts(&d)),
        Err(UpdateError::WrongDevice)
    ));
    assert!(matches!(
        check(&m, |f| f.channel = Some("beta")),
        Err(UpdateError::WrongChannel)
    ));
    assert!(check(&m, |f| f.channel = None).is_ok());
}

#[test]
fn payload_roles_must_equal_the_slot_layout() {
    let m = sample();
    for roles in [
        &[Role::Kernel][..],
        &[Role::System],
        &[Role::Kernel, Role::System, Role::Boot],
        &[],
    ] {
        assert!(
            matches!(
                check(&m, |f| f.roles = roles),
                Err(UpdateError::PayloadSetMismatch)
            ),
            "{roles:?}"
        );
    }
    assert!(check(&m, |f| f.roles =
        &[Role::System, Role::Kernel, Role::System])
    .is_ok());
}

#[test]
fn rollback_index_and_versions() {
    let m = sample();
    assert!(check(&m, |f| f.floor = 3).is_ok(), "equal index is allowed");
    assert!(matches!(
        check(&m, |f| f.floor = 4),
        Err(UpdateError::RollbackRefused {
            floor: 4,
            offered: 3
        })
    ));
    assert!(matches!(
        check(&m, |f| f.current = Version::new(0, 6, 9)),
        Err(UpdateError::NeedsIntermediate)
    ));
    assert!(matches!(
        check(&m, |f| f.current = Version::new(0, 7, 1)),
        Err(UpdateError::UpToDate)
    ));
    assert!(matches!(
        check(&m, |f| f.current = Version::new(0, 8, 0)),
        Err(UpdateError::DowngradeRefused)
    ));
    assert!(check(&m, |f| {
        f.current = Version::new(0, 8, 0);
        f.allow_downgrade = true;
    })
    .is_ok());
    // Even an allowed downgrade never goes below the floor.
    assert!(matches!(
        check(&m, |f| {
            f.current = Version::new(0, 8, 0);
            f.allow_downgrade = true;
            f.floor = 9;
        }),
        Err(UpdateError::RollbackRefused { .. })
    ));
}

#[test]
fn first_failure_wins_in_documented_order() {
    let mut m = sample();
    m.arch = "aarch64".into();
    m.edition = "phone".into();
    assert!(matches!(
        check(&m, |f| {
            f.now = u64::MAX;
            f.floor = 99;
        }),
        Err(UpdateError::Expired)
    ));
    assert!(matches!(
        check(&m, |f| f.floor = 99),
        Err(UpdateError::WrongArch)
    ));
}
