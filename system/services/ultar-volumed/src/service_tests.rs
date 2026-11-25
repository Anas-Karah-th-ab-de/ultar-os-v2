//! Unit tests of the drive service itself (included as `service::tests`).

use super::*;

#[test]
fn labels_become_safe_folder_names() {
    assert_eq!(folder("STICK", "sdb1"), "STICK");
    assert_eq!(folder("../../etc", "sdb1"), "etc");
    assert_eq!(folder("", "sdb1"), "sdb1");
    assert_eq!(folder("/ /", "sdc"), "sdc");
    assert_eq!(folder("صور", "sdb1"), "صور");
    assert!(plain("amal") && !plain("../x"));
}

#[test]
fn eject_of_an_unknown_drive() {
    let mut s = VolumeService::new("/nonexistent".into(), "/nonexistent".into());
    assert_eq!(s.eject("sdb1"), EjectOutcome::Unknown);
    assert_eq!(s.state(), VolumesState::default());
}

#[test]
fn usage_measures_the_own_storage_first() {
    let dir = std::env::temp_dir();
    let s = VolumeService::new(dir.clone(), "/nonexistent".into());
    let usage = s.usage();
    assert_eq!(usage.places.len(), 1, "no drives mounted");
    let own = &usage.places[0];
    assert_eq!((own.id.as_str(), own.name.as_str()), (INTERNAL, ""));
    assert!(own.total_bytes > 0 && own.free_bytes <= own.total_bytes);
    assert_eq!(
        space_of(&dir).map(|(total, _)| total),
        Some(own.total_bytes)
    );
    // A place that cannot be measured is left out.
    let gone = VolumeService::new("/nonexistent/ultar".into(), "/nonexistent".into());
    assert!(gone.usage().places.is_empty());
}
