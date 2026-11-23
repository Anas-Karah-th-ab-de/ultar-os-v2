//! Request dispatch of the bus service: malformed requests never stop the daemon.

mod rig;

use rig::kernel_fmt::beelink;
use rig::Rig;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::device_service::{List, Shutdown};
use ultar_bus::{
    Broker, BusError, Client, ClientOptions, ErrorCode, FixedIdentity, Identity, Policy,
};
use ultar_devd::daemon::Daemon;

const WAIT: Duration = Duration::from_secs(30);

#[test]
fn malformed_requests_get_typed_errors_and_do_not_stop_the_service() {
    let rig = Rig::new("service");
    let machine = beelink();
    machine.write_ultar_root(&rig.root);
    rig.standard_depot();
    let socket = rig.root.parent().unwrap().join("bus.sock");
    let identity = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let _broker: Broker = Broker::builder(Policy::allow_all())
        .listen(&socket, identity)
        .start()
        .unwrap();
    let bus = Client::connect(&socket, ClientOptions::named("ultar-devd")).unwrap();
    let endpoint = Daemon::register(&bus).unwrap();
    let mut daemon = Daemon::start(rig.config(), Box::new(bus.clone())).unwrap();
    let server = std::thread::spawn(move || daemon.serve(&endpoint));
    let caller = Client::connect(&socket, ClientOptions::named("caller")).unwrap();
    let raw = |method: &str, payload: &[u8]| -> Result<(), BusError> {
        caller.request("devices", method, &payload.to_vec(), WAIT)
    };
    // Untyped requests carry contract version 0: refused, also for `shutdown`.
    for method in [
        "shutdown",
        "switch",
        "list",
        "rescan",
        "rollback",
        "clear_quarantine",
    ] {
        let result = raw(method, b"\xff\xff\xff");
        let refused = matches!(
            result,
            Err(BusError::Remote(
                ErrorCode::VersionMismatch | ErrorCode::BadPayload
            ))
        );
        assert!(refused, "{method}: {result:?}");
    }
    assert!(matches!(
        raw("reboot", b""),
        Err(BusError::Remote(ErrorCode::UnknownMethod))
    ));
    // The service is still there and stops only on a proper request.
    assert_eq!(caller.call(&List {}, WAIT).unwrap().devices.len(), 14);
    caller.call(&Shutdown {}, WAIT).unwrap();
    assert!(server.join().unwrap().is_ok());
}
