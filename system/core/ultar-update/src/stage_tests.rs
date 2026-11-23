use super::*;
use crate::manifest::tests::sample;

fn manifest_for(parts: &[&[u8]]) -> Manifest {
    let mut manifest = sample();
    for (payload, data) in manifest.payloads.iter_mut().zip(parts) {
        payload.size = data.len() as u64;
        payload.sha256 = Sha256::digest(data).into();
    }
    manifest
}

#[test]
fn payload_check_at_every_split_point() {
    let (kernel, system) = (b"kernel-bytes".as_slice(), b"the system image".as_slice());
    let manifest = manifest_for(&[kernel, system]);
    let stream = [kernel, system].concat();
    for split in 0..=stream.len() {
        let mut check = PayloadCheck::new(&manifest);
        check.feed(&stream[..split]).unwrap();
        assert_eq!(check.complete(), split == stream.len());
        check.feed(&stream[split..]).unwrap();
        assert!(check.complete());
        assert!(matches!(check.feed(b"x"), Err(UpdateError::SizeExceeded)));
    }
}

#[test]
fn payload_check_names_the_wrong_payload_as_soon_as_it_ends() {
    let (kernel, system) = (b"kernel-bytes".as_slice(), b"the system image".as_slice());
    let manifest = manifest_for(&[kernel, system]);
    let mut check = PayloadCheck::new(&manifest);
    assert!(matches!(
        check.feed(b"kernel-bytez"),
        Err(UpdateError::HashMismatch(Role::Kernel))
    ));
    let mut check = PayloadCheck::new(&manifest);
    check.feed(kernel).unwrap();
    check.feed(&system[..5]).unwrap();
    assert!(matches!(
        check.feed(b"XXXXXXXXXXX"),
        Err(UpdateError::HashMismatch(Role::System))
    ));
}

#[test]
fn marker_is_strict() {
    let info = StagedInfo {
        digest: [0xAB; 32],
        size: 123_456,
        allow_downgrade: true,
    };
    let text = marker_text(&info);
    assert_eq!(parse_marker(text.as_bytes()), Some(info));
    for bad in [
        text.trim_end().to_string(),
        text.replace(" 1\n", " 2\n"),
        text.replace(" 1\n", "\n"),
        text.replace("123456", "0123456"),
        text.replace("ab", "AB"),
        format!("{text}x"),
        String::new(),
    ] {
        assert_eq!(parse_marker(bad.as_bytes()), None, "{bad:?}");
    }
}

#[test]
fn marker_without_matching_file_is_not_a_staged_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let staging = Staging::new(dir.path());
    assert_eq!(staging.staged().unwrap(), None);
    let info = StagedInfo {
        digest: [1; 32],
        size: 10,
        allow_downgrade: false,
    };
    fs::write(staging.marker(), marker_text(&info)).unwrap();
    assert_eq!(staging.staged().unwrap(), None, "marker alone");
    fs::write(staging.done(), b"123456789").unwrap();
    assert_eq!(staging.staged().unwrap(), None, "size differs");
    fs::write(staging.done(), b"1234567890").unwrap();
    assert_eq!(staging.staged().unwrap(), Some(info));
    assert!(staging.open_staged().unwrap().is_some());
    staging.purge().unwrap();
    assert_eq!(staging.staged().unwrap(), None);
    assert_eq!(staging.partial_bytes(), 0);
    staging.purge().unwrap();
}

#[test]
fn recorded_length_never_exceeds_what_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let staging = Staging::new(dir.path());
    fs::write(staging.part(), b"12345").unwrap();
    assert_eq!(staging.partial_bytes(), 0, "nothing recorded as flushed");
    fs::write(staging.pos(), b"3").unwrap();
    assert_eq!(staging.partial_bytes(), 3);
    fs::write(staging.pos(), b"900").unwrap();
    assert_eq!(staging.partial_bytes(), 5);
    fs::write(staging.pos(), b"garbage").unwrap();
    assert_eq!(staging.partial_bytes(), 0);
}
