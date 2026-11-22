//! Exercises the power, sensor, storage and TPM traits through the mock backend,
//! including error paths and scripted failures.

use ultar_hal::mock::{MockBackend, MockPower, PowerAction};
use ultar_hal::{
    Backend, BlockDeviceId, ChargeStatus, ChargerKind, ChargerState, HalError, PowerProfile,
    RebootTarget, SensorId, SensorKind, TpmVersion, TPM_DIGEST_LEN,
};

#[test]
fn power_battery_charger_and_zones() {
    let phone = MockBackend::phone();
    let power = phone.power().unwrap();
    let battery = power.battery().unwrap().unwrap();
    assert_eq!(battery.percent, 80);
    assert_eq!(battery.status, ChargeStatus::Discharging);
    assert!(!battery.is_low(15));

    let mock = &phone.base.power;
    let mut low = MockPower::discharging_battery(250);
    assert_eq!(low.percent, 100);
    low.percent = 9;
    mock.set_battery(Some(low));
    assert!(power.battery().unwrap().unwrap().is_low(15));
    mock.set_battery(None);
    assert_eq!(power.battery().unwrap(), None);

    assert!(power.charger().unwrap().is_connected());
    mock.set_charger(ChargerState {
        kind: ChargerKind::None,
        max_power_mw: None,
    });
    assert!(!power.charger().unwrap().is_connected());

    assert_eq!(power.thermal_zones().unwrap().len(), 1);
    mock.set_zone("cpu", 70_000, Some(95_000));
    mock.set_zone("cpu", 96_000, Some(95_000));
    let zones = power.thermal_zones().unwrap();
    assert_eq!(zones.len(), 2);
    let cpu = zones.iter().find(|zone| zone.name == "cpu").unwrap();
    assert!(cpu.is_critical());
}

#[test]
fn power_profiles_and_state_changes() {
    let server = MockBackend::server();
    let power = server.power().unwrap();
    assert_eq!(power.battery().unwrap(), None);
    assert_eq!(power.power_profile().unwrap(), PowerProfile::Balanced);
    power.set_power_profile(PowerProfile::Performance).unwrap();
    assert_eq!(power.power_profile().unwrap(), PowerProfile::Performance);

    power.suspend().unwrap();
    power.reboot(RebootTarget::Recovery).unwrap();
    server.base.power.fail_next(HalError::PermissionDenied);
    assert_eq!(power.power_off(), Err(HalError::PermissionDenied));
    power.power_off().unwrap();
    assert_eq!(
        server.base.power.actions(),
        vec![
            PowerAction::Suspend,
            PowerAction::Reboot(RebootTarget::Recovery),
            PowerAction::PowerOff
        ]
    );
}

#[test]
fn sensors_list_read_and_disable() {
    let phone = MockBackend::phone();
    let sensors = phone.sensors().unwrap();
    assert_eq!(sensors.list(None).unwrap().len(), 4);
    let light = sensors.list(Some(SensorKind::Light)).unwrap();
    assert_eq!(light.len(), 1);
    assert!(sensors.list(Some(SensorKind::Hinge)).unwrap().is_empty());
    let accelerometer = sensors.list(Some(SensorKind::Accelerometer)).unwrap()[0].id;

    let first = sensors.read(accelerometer).unwrap();
    assert_eq!(first.values(), &[0.0, 9.81, 0.0]);
    let lux = sensors.read(light[0].id).unwrap();
    assert_eq!(lux.values(), &[320.0]);
    assert!(lux.time_us > first.time_us);

    let mock = &phone.base.sensors;
    mock.set_values(accelerometer, [9.81, 0.0, 0.0]).unwrap();
    assert_eq!(sensors.read(accelerometer).unwrap().raw[0], 9.81);

    assert!(mock.is_enabled(accelerometer));
    sensors.set_enabled(accelerometer, false).unwrap();
    assert!(!mock.is_enabled(accelerometer));
    assert_eq!(sensors.read(accelerometer), Err(HalError::Rejected));
    sensors.set_enabled(accelerometer, true).unwrap();
    assert!(sensors.read(accelerometer).is_ok());
}

#[test]
fn sensor_errors() {
    let phone = MockBackend::phone();
    let sensors = phone.sensors().unwrap();
    let mock = &phone.base.sensors;
    let missing = SensorId(99);
    assert_eq!(sensors.read(missing), Err(HalError::NotFound));
    assert_eq!(sensors.set_enabled(missing, true), Err(HalError::NotFound));
    assert_eq!(mock.set_values(missing, [0.0; 3]), Err(HalError::NotFound));
    assert!(!mock.is_enabled(missing));
    mock.fail_next(HalError::Io(5));
    assert_eq!(sensors.list(None), Err(HalError::Io(5)));
}

#[test]
fn storage_read_write_round_trip() {
    let server = MockBackend::server();
    let storage = server.storage().unwrap();
    let devices = storage.devices().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].capacity_bytes(), 512 * 4_096);
    let disk = devices[0].id;

    let pattern: Vec<u8> = (0..1024).map(|index| (index % 251) as u8).collect();
    storage.write_blocks(disk, 10, &pattern).unwrap();
    let mut read_back = vec![0; 1024];
    storage.read_blocks(disk, 10, &mut read_back).unwrap();
    assert_eq!(read_back, pattern);
    // The block before is untouched; the second written block reads on its own.
    let mut single = vec![0xff; 512];
    storage.read_blocks(disk, 9, &mut single).unwrap();
    assert!(single.iter().all(|byte| *byte == 0));
    storage.read_blocks(disk, 11, &mut single).unwrap();
    assert_eq!(single, pattern[512..]);
    // The last block is reachable, an empty transfer is fine.
    storage.read_blocks(disk, 4_095, &mut single).unwrap();
    storage.read_blocks(disk, 0, &mut []).unwrap();

    storage.flush(disk).unwrap();
    storage.flush(disk).unwrap();
    assert_eq!(server.base.storage.flushes(disk), 2);
}

#[test]
fn storage_errors() {
    let server = MockBackend::server();
    let storage = server.storage().unwrap();
    let devices = storage.devices().unwrap();
    let (disk, read_only) = (devices[0].id, devices[1].id);
    assert!(devices[1].read_only);
    let mut block = vec![0; 512];

    assert_eq!(
        storage.read_blocks(disk, 0, &mut block[..100]),
        Err(HalError::InvalidArgument)
    );
    assert_eq!(
        storage.read_blocks(disk, 4_096, &mut block),
        Err(HalError::InvalidArgument)
    );
    assert_eq!(
        storage.read_blocks(disk, u64::MAX, &mut block),
        Err(HalError::InvalidArgument)
    );
    assert_eq!(
        storage.write_blocks(disk, 4_095, &[0; 1024]),
        Err(HalError::InvalidArgument)
    );
    assert_eq!(
        storage.write_blocks(read_only, 0, &block),
        Err(HalError::PermissionDenied)
    );
    storage.read_blocks(read_only, 0, &mut block).unwrap();

    let missing = BlockDeviceId(9);
    assert_eq!(
        storage.read_blocks(missing, 0, &mut block),
        Err(HalError::NotFound)
    );
    assert_eq!(
        storage.write_blocks(missing, 0, &block),
        Err(HalError::NotFound)
    );
    assert_eq!(storage.flush(missing), Err(HalError::NotFound));
    assert_eq!(server.base.storage.flushes(missing), 0);
    server.base.storage.fail_next(HalError::Io(5));
    assert_eq!(storage.flush(disk), Err(HalError::Io(5)));
    // A block size of zero is replaced by 512.
    let odd = server.base.storage.add_device("odd", 0, 1, false);
    let info = storage.devices().unwrap().pop().unwrap();
    assert_eq!((info.id, info.block_size), (odd, 512));
}

#[test]
fn tpm_random_is_deterministic_per_instance() {
    let first = MockBackend::laptop();
    let second = MockBackend::laptop();
    let mut a = [0u8; 16];
    let mut b = [0u8; 16];
    first.tpm().unwrap().random(&mut a).unwrap();
    second.tpm().unwrap().random(&mut b).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, [0u8; 16]);
    first.tpm().unwrap().random(&mut b).unwrap();
    assert_ne!(a, b);
    assert_eq!(first.tpm().unwrap().version().unwrap(), TpmVersion::V2_0);
}

#[test]
fn tpm_registers_extend() {
    let laptop = MockBackend::laptop();
    let tpm = laptop.tpm().unwrap();
    let zero = [0u8; TPM_DIGEST_LEN];
    assert_eq!(tpm.pcr_read(7).unwrap(), zero);
    let measurement = [0xab; TPM_DIGEST_LEN];
    tpm.pcr_extend(7, &measurement).unwrap();
    let once = tpm.pcr_read(7).unwrap();
    assert_ne!(once, zero);
    tpm.pcr_extend(7, &measurement).unwrap();
    assert_ne!(tpm.pcr_read(7).unwrap(), once);
    assert_eq!(tpm.pcr_read(8).unwrap(), zero);
    assert_eq!(tpm.pcr_read(23).unwrap(), zero);
    assert_eq!(tpm.pcr_read(24), Err(HalError::InvalidArgument));
    assert_eq!(
        tpm.pcr_extend(24, &measurement),
        Err(HalError::InvalidArgument)
    );
}

#[test]
fn tpm_sealing_is_bound_to_registers() {
    let laptop = MockBackend::laptop();
    let tpm = laptop.tpm().unwrap();
    let secret = b"disk encryption key material";
    let mask = (1 << 0) | (1 << 7);
    let sealed = tpm.seal(secret, mask).unwrap();
    assert!(!sealed.windows(secret.len()).any(|part| part == secret));
    assert_eq!(tpm.unseal(&sealed).unwrap(), secret);
    assert_eq!(tpm.unseal(&tpm.seal(b"", 0).unwrap()).unwrap(), b"");

    // A register outside the mask may change, one inside may not.
    tpm.pcr_extend(3, &[1; TPM_DIGEST_LEN]).unwrap();
    assert_eq!(tpm.unseal(&sealed).unwrap(), secret);
    tpm.pcr_extend(7, &[1; TPM_DIGEST_LEN]).unwrap();
    assert_eq!(tpm.unseal(&sealed), Err(HalError::Rejected));

    assert_eq!(tpm.unseal(b"short"), Err(HalError::Rejected));
    assert_eq!(tpm.unseal(&[0; 64]), Err(HalError::Rejected));
    assert_eq!(tpm.seal(secret, 1 << 24), Err(HalError::InvalidArgument));
    // A blob from another module state does not open here.
    let other = MockBackend::laptop();
    other
        .tpm()
        .unwrap()
        .pcr_extend(0, &[9; TPM_DIGEST_LEN])
        .unwrap();
    let foreign = other.tpm().unwrap().seal(secret, mask).unwrap();
    assert_eq!(tpm.unseal(&foreign), Err(HalError::Rejected));
    laptop.base.tpm.fail_next(HalError::Busy);
    assert_eq!(tpm.version(), Err(HalError::Busy));
}
