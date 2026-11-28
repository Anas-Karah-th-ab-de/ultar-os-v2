//! A release larger than one written file may be on the Ultar kernel (64 MiB): the staging area
//! keeps it in segments, a download that breaks off across a segment border resumes, and the
//! release installs. (0.8.1 and 0.8.2 staged the bundle as one file and failed with "could not
//! read from or write to the storage medium".)

mod common;

use common::*;
use ultar_update::bootctl::Slot;
use ultar_update::segfile::SEGMENT;
use ultar_update::stage::NoPacing;

/// A release whose system image fills two segments and a bit.
fn big_release() -> Release {
    let small = release((0, 7, 1), 1);
    let mut manifest = small.manifest.clone();
    let system = Rng(0x5EED).bytes(2 * SEGMENT as usize + 777);
    manifest.payloads[1].size = system.len() as u64;
    manifest.payloads[1].sha256 = sha(&system);
    let payloads = vec![small.payloads[0].clone(), system];
    assemble(manifest, payloads, &[], &PUBLISHER)
}

#[test]
fn a_release_beyond_the_file_limit_is_staged_in_segments_and_installs() {
    let device = Device::new();
    let new = big_release();
    let mut engine = device.engine();
    let mut source = MemSource::new(&new.bundle);
    // past the first flush of the second segment
    source.cut_after = Some(SEGMENT as usize + 5 * 1024 * 1024 + 123);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    assert!(engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .is_err());
    source.cut_after = None;
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    let resumed_at = *source.opens.last().unwrap();
    assert!(
        resumed_at > SEGMENT,
        "resumed, not started again: {resumed_at}"
    );
    let staging = device.config.state_dir.join("staging");
    let mut segments = 0;
    for entry in std::fs::read_dir(&staging).unwrap() {
        let entry = entry.unwrap();
        let len = entry.metadata().unwrap().len();
        assert!(len <= SEGMENT, "{:?} holds {len} bytes", entry.file_name());
        segments += usize::from(
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("bundle.uup."),
        );
    }
    assert_eq!(segments, 3);
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
}
