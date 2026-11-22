//! Store failures never stop the daemon.

mod common;

use common::{boot_with, key, nic, three_drivers, FlakyStore, ABI, NIC};
use ultar_devd::{Config, DeviceEvent, Phase, StoreError};

#[test]
fn store_failures_do_not_stop_the_daemon() {
    let index = three_drivers();
    // Unreadable store at boot: start without memory.
    let store = FlakyStore {
        fail_load: Some(StoreError::Corrupt),
        ..FlakyStore::default()
    };
    let fail_save = store.fail_save.clone();
    let mut rig = boot_with(&index, store, Config::new(ABI));
    assert_eq!(rig.manager.store_error(), Some(StoreError::Corrupt));
    let report = rig.add(nic());
    assert_eq!(report.active, Some(key("alpha")));
    // The first successful save clears the error (the corrupt content was replaced).
    assert_eq!(report.store_error, None);
    assert_eq!(rig.manager.store_error(), None);
    assert_eq!(rig.manager.store().inner.saves(), 1);

    // Writing fails: decisions still apply in memory and the report says so.
    *fail_save.borrow_mut() = Some(StoreError::WriteFailed);
    let report = rig.manager.switch_driver(NIC, &key("beta")).unwrap();
    assert_eq!(report.active, Some(key("beta")));
    assert_eq!(report.store_error, Some(StoreError::WriteFailed));
    assert_eq!(
        rig.manager.state().choices.get(&nic().identity()),
        Some(&key("beta"))
    );
    assert_eq!(
        rig.manager
            .store()
            .inner
            .state()
            .choices
            .get(&nic().identity()),
        Some(&key("alpha"))
    );
    assert_eq!(
        rig.manager.roll_back(NIC).unwrap().store_error,
        Some(StoreError::WriteFailed)
    );

    // Storage is back: the next change writes everything that was pending.
    *fail_save.borrow_mut() = None;
    let report = rig.manager.switch_driver(NIC, &key("gamma")).unwrap();
    assert_eq!(report.store_error, None);
    assert_eq!(rig.manager.store().inner.state(), rig.manager.state());
}

#[test]
fn without_a_data_partition_decisions_last_until_shutdown() {
    let index = three_drivers();
    let store = FlakyStore {
        fail_load: Some(StoreError::Unavailable),
        ..FlakyStore::default()
    };
    *store.fail_save.borrow_mut() = Some(StoreError::Unavailable);
    let mut rig = boot_with(&index, store, Config::new(ABI));
    let report = rig.add(nic());
    assert_eq!(
        (report.active, report.store_error),
        (Some(key("alpha")), Some(StoreError::Unavailable))
    );
    let removed = rig
        .manager
        .handle_event(DeviceEvent::Removed(NIC.into()))
        .unwrap();
    assert_eq!(removed.final_phase(), Some(Phase::Removed));
}
