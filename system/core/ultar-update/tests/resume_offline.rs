//! Resuming downloads, and updates from a stick or folder without any network.

mod common;

use common::*;
use ultar_update::bootctl::Slot;
use ultar_update::commit::BootOutcome;
use ultar_update::engine::EnginePhase;
use ultar_update::error::UpdateError;
use ultar_update::stage::NoPacing;
use ultar_update::version::Version;

#[test]
fn download_resumes_where_the_connection_broke() {
    let device = Device::new();
    let new = release((0, 7, 1), 1);
    let header = new.header.raw.len() as u64;
    let mut engine = device.engine();
    let mut source = MemSource::new(&new.bundle);
    source.cut_after = Some(70_000);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    let mut rounds = 0;
    loop {
        rounds += 1;
        match engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {}) {
            Ok(_) => break,
            Err(UpdateError::Truncated) => {
                assert_eq!(engine.status().unwrap().phase, EnginePhase::Partial);
            }
            Err(other) => panic!("{other}"),
        }
        assert!(rounds < 20);
    }
    // First open at 0 is the header probe; then each stream continues exactly after the last.
    let body_opens: Vec<u64> = source.opens[1..].to_vec();
    let expected: Vec<u64> = (0..body_opens.len() as u64)
        .map(|i| header + i * 70_000)
        .collect();
    assert_eq!(body_opens, expected);
    assert_eq!(rounds as usize, body_opens.len());
    assert!(rounds >= 4);
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.images(Slot::B), new.payloads);
}

#[test]
fn resume_survives_a_restart_of_the_updater() {
    let device = Device::new();
    let new = release((0, 7, 1), 1);
    let mut source = MemSource::new(&new.bundle);
    source.cut_after = Some(150_000);
    {
        let mut engine = device.engine();
        let offer = engine.check(&mut source, "stable").unwrap().unwrap();
        assert!(matches!(
            engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {}),
            Err(UpdateError::Truncated)
        ));
    }
    source.cut_after = None;
    let mut engine = device.engine();
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    let mut first_progress = None;
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |done, _| {
            first_progress.get_or_insert(done);
        })
        .unwrap();
    assert_eq!(first_progress, Some(new.header.raw.len() as u64 + 150_000));
    assert_eq!(
        *source.opens.last().unwrap(),
        new.header.raw.len() as u64 + 150_000
    );
}

#[test]
fn source_without_range_support_restarts_cleanly() {
    let device = Device::new();
    let new = release((0, 7, 1), 1);
    let mut engine = device.engine();
    let mut source = MemSource::new(&new.bundle);
    source.cut_after = Some(100_000);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    assert!(engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .is_err());
    source.resumable = false; // answers every request from byte 0
    source.cut_after = None;
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.images(Slot::B), new.payloads);
}

#[test]
fn partial_download_of_another_release_is_discarded() {
    let device = Device::new();
    let first = release((0, 7, 1), 1);
    let second = release((0, 7, 2), 1);
    let mut engine = device.engine();
    let mut source = MemSource::new(&first.bundle);
    source.cut_after = Some(100_000);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    assert!(engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .is_err());
    // The server publishes a newer release before the download is continued.
    let mut source = MemSource::new(&second.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    assert_eq!(
        source.opens,
        [0, second.header.raw.len() as u64],
        "started from the first payload byte"
    );
    assert_eq!(
        engine.status().unwrap().pending_version,
        Some(second.manifest.version)
    );
}

#[test]
fn damaged_partial_file_is_detected_on_resume() {
    let device = Device::new();
    let new = release((0, 7, 1), 1);
    let mut engine = device.engine();
    let mut source = MemSource::new(&new.bundle);
    source.cut_after = Some(KERNEL_LEN + 50_000); // kernel complete, system partly
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    assert!(engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .is_err());
    let part = device.config.state_dir.join("staging/bundle.part.000");
    let mut bytes = std::fs::read(&part).unwrap();
    bytes[new.header.raw.len() + 10] ^= 1;
    std::fs::write(&part, bytes).unwrap();
    source.cut_after = None;
    assert!(matches!(
        engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {}),
        Err(UpdateError::HashMismatch(
            ultar_update::manifest::Role::Kernel
        ))
    ));
    assert_eq!(engine.status().unwrap().phase, EnginePhase::Idle);
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
}

#[test]
fn rate_limit_is_applied_to_the_download() {
    use ultar_update::stage::Pacer;
    struct Count(u64);
    impl Pacer for Count {
        fn consumed(&mut self, bytes: u64) {
            self.0 += bytes;
        }
    }
    let device = Device::new();
    let new = release((0, 7, 1), 1);
    let mut engine = device.engine();
    let mut source = MemSource::new(&new.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    let mut pacer = Count(0);
    engine
        .download(&mut source, &offer, &mut pacer, &mut |_, _| {})
        .unwrap();
    assert_eq!(pacer.0, (KERNEL_LEN + SYSTEM_LEN) as u64);
}

#[test]
fn offline_update_from_a_single_file() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let stick = device.write_bundle("ultar-0.7.1.uup", &new);
    let mut engine = device.engine();
    let offer = engine.stage_offline(&stick, false, &mut |_, _| {}).unwrap();
    assert_eq!(offer.verified.manifest().version, new.manifest.version);
    std::fs::remove_file(&stick).unwrap(); // the stick is pulled: the staged copy is enough
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    let mut engine = device.engine();
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::Committed {
            version: Some(new.manifest.version)
        }
    );
}

#[test]
fn offline_folder_picks_the_newest_acceptable_bundle() {
    let device = Device::new();
    let older = release((0, 7, 1), 1);
    let newest = release((0, 7, 3), 1);
    let for_phone = release_with((0, 9, 0), 1, &PUBLISHER, |m| m.edition = "phone".into());
    let foreign = release_with((0, 9, 9), 1, &STRANGER, |_| {});
    device.write_bundle("a.uup", &older);
    device.write_bundle("b.uup", &for_phone);
    device.write_bundle("c.uup", &newest);
    device.write_bundle("d.uup", &foreign);
    let folder = device
        .write_bundle("e.uup", &older)
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::write(folder.join("junk.uup"), b"not a bundle at all").unwrap();
    std::fs::write(folder.join("readme.txt"), b"hello").unwrap();
    let mut engine = device.engine();
    let offer = engine
        .stage_offline(&folder, false, &mut |_, _| {})
        .unwrap();
    assert_eq!(offer.verified.manifest().version, Version::new(0, 7, 3));
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.images(Slot::B), newest.payloads);
}

#[test]
fn offline_failures_are_the_same_as_online() {
    let device = Device::new();
    let mut engine = device.engine();
    let empty = device.dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        engine.stage_offline(&empty, false, &mut |_, _| {}),
        Err(UpdateError::NoUpdate)
    ));
    let foreign = release_with((0, 7, 1), 1, &STRANGER, |_| {});
    let path = device.write_bundle("x.uup", &foreign);
    assert!(matches!(
        engine.stage_offline(&path, false, &mut |_, _| {}),
        Err(UpdateError::UnknownKey)
    ));
    let mut damaged = release((0, 7, 1), 1);
    let at = damaged.bundle.len() - 9;
    damaged.bundle[at] ^= 2;
    let path = device.write_bundle("y.uup", &damaged);
    assert!(matches!(
        engine.stage_offline(&path, false, &mut |_, _| {}),
        Err(UpdateError::HashMismatch(_))
    ));
    let mut short = release((0, 7, 1), 1);
    short.bundle.truncate(short.bundle.len() - 1);
    let path = device.write_bundle("z.uup", &short);
    assert!(matches!(
        engine.stage_offline(&path, false, &mut |_, _| {}),
        Err(UpdateError::Truncated)
    ));
    assert!(device.slot_b_untouched());
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::NotStaged)
    ));
}

#[test]
fn offline_downgrade_needs_permission_and_stays_above_the_floor() {
    let device = Device::new();
    let v2 = release((0, 7, 2), 1);
    install(&mut device.engine(), &v2).unwrap();
    device.reboot(&[&v2]);
    device.engine().resume_after_boot(&Health(true)).unwrap();
    let v1 = release((0, 7, 1), 1);
    let path = device.write_bundle("v1.uup", &v1);
    let mut engine = device.engine();
    assert!(matches!(
        engine.stage_offline(&path, false, &mut |_, _| {}),
        Err(UpdateError::DowngradeRefused)
    ));
    engine.stage_offline(&path, true, &mut |_, _| {}).unwrap();
    // The permission is remembered with the staged bundle (apply verifies everything again).
    let mut engine = device.engine();
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.reboot(&[&v2, &v1]), (Slot::A, 2));
}
