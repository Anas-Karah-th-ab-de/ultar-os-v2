//! Self check of the bus on the running kernel: broker and two clients in one process.
//!
//! Used by the kernel test (`tools/kern-test`) to prove that Unix sockets, threads and
//! blocking I/O of the Ultar kernel carry the bus. Prints `BUS SELFCHECK OK` on success.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::power::{PowerProfile, PowerSource, PowerState, ThermalLevel};
use ultar_bus::{Broker, BusError, Client, ClientOptions, FixedIdentity, Identity, Policy};

const WAIT: Duration = Duration::from_secs(10);

fn run() -> Result<(), BusError> {
    let path =
        std::env::temp_dir().join(format!("ultar-bus-selfcheck-{}.sock", std::process::id()));
    let identity = Arc::new(FixedIdentity::new(Identity::new("selfcheck")?));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&path, identity)
        .start()?;

    let publisher = Client::connect(&path, ClientOptions::named("publisher"))?;
    let state = PowerState {
        source: PowerSource::Battery,
        battery_percent: Some(87),
        charging: false,
        profile: PowerProfile::Balanced,
        thermal: ThermalLevel::Normal,
    };
    publisher.publish(&state)?;

    // A late subscriber must receive the retained value at once.
    let listener = Client::connect(&path, ClientOptions::named("listener"))?;
    let subscription = listener.subscribe::<PowerState>()?;
    let retained = subscription.recv_timeout(WAIT)?;
    if retained != state {
        return Err(BusError::Closed);
    }
    // And every later value in order.
    for percent in [86u8, 85, 84] {
        publisher.publish(&PowerState {
            battery_percent: Some(percent),
            ..state
        })?;
    }
    for percent in [86u8, 85, 84] {
        if subscription.recv_timeout(WAIT)?.battery_percent != Some(percent) {
            return Err(BusError::Closed);
        }
    }
    listener.close();
    publisher.close();
    broker.shutdown();
    let _ = std::fs::remove_file(&path);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("BUS SELFCHECK OK");
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("BUS SELFCHECK FAILED: {error:?}");
            ExitCode::FAILURE
        }
    }
}
