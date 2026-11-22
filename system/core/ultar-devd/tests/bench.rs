//! Measurement only (ignored by default):
//! `cargo test --release --test bench -- --ignored --nocapture`.

mod rig;

use rig::kernel_fmt::{beelink, thinkpad, write, ULTAR_VERSION};
use rig::Rig;
use std::time::Instant;

fn measure(name: &str, rig: &Rig) {
    const RUNS: u32 = 20;
    let (first, _) = rig.start();
    let devices = first.list().devices.len();
    drop(first);
    let started = Instant::now();
    for _ in 0..RUNS {
        let (daemon, _) = rig.start();
        assert_eq!(daemon.list().devices.len(), devices);
    }
    let per_start = started.elapsed() / RUNS;
    println!("{name}: {devices} devices, start with remembered choices {per_start:?}");
}

#[test]
#[ignore = "measurement, not a check"]
fn startup_time() {
    let rig = Rig::new("bench-beelink");
    rig.standard_depot();
    let machine = beelink();
    machine.write_ultar_root(&rig.root, &machine.selftest_log());
    measure("beelink (ultar kernel log)", &rig);
    let rig = Rig::new("bench-thinkpad");
    rig.standard_depot();
    thinkpad().write_sysfs_root(&rig.root, true);
    write(&rig.root, "proc/version", ULTAR_VERSION);
    measure("thinkpad (sysfs)", &rig);
}
