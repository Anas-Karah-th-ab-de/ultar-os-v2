//! The Ultar account link on its own thread of the daemon: talking to the server
//! takes up to a few seconds and must not hold up the local users. Split from
//! [`crate::daemon`] so that file stays the command line and the service loop.

use crate::daemon::Args;
use crate::link::{Link, LinkCall};
use crate::link_cloud::CloudServer;
use crate::link_store::Store;
use std::time::Instant;
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_service::BusSession;

/// The kind word of this device for the account's device list.
fn device_kind() -> String {
    match std::env::var("ULTAR_EDITION").as_deref() {
        Ok("phone") => "phone".into(),
        Ok("server") => "server".into(),
        _ => "desktop".into(),
    }
}

/// This device's name as the account lists it.
fn device_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().chars().take(64).collect::<String>())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "Ultar".into())
}

fn decode_link(r: &IncomingRequest) -> Option<LinkCall> {
    use ultar_bus::messages::account_service as a;
    let m = r.method();
    Some(match m {
        _ if m == a::Status::METHOD => LinkCall::Status,
        _ if m == a::SignOut::METHOD => LinkCall::SignOut,
        _ if m == a::SignIn::METHOD => LinkCall::SignIn(r.decode().ok()?),
        _ if m == a::Create::METHOD => LinkCall::Create(r.decode().ok()?),
        _ if m == a::SetServer::METHOD => LinkCall::SetServer(r.decode().ok()?),
        _ if m == a::SetService::METHOD => LinkCall::SetService(r.decode().ok()?),
        _ if m == a::ChangePassword::METHOD => LinkCall::ChangePassword(r.decode().ok()?),
        _ if m == a::Delete::METHOD => LinkCall::Delete(r.decode().ok()?),
        _ if m == a::DeviceSignOut::METHOD => LinkCall::DeviceSignOut(r.decode().ok()?),
        _ if m == a::DeviceRemove::METHOD => LinkCall::DeviceRemove(r.decode().ok()?),
        _ => return None,
    })
}

/// Runs the link until the calls channel ends (the service loop dropped it).
pub(crate) fn serve_link(
    args: &Args,
    calls: std::sync::mpsc::Receiver<Box<IncomingRequest>>,
    session: &BusSession,
) {
    let store = Store::new(&args.dir, args.relay_token.clone(), args.relay_group);
    let mut link = Link::open(
        CloudServer::new(&args.dir),
        &args.account_server,
        store,
        &device_name(),
        &device_kind(),
    );
    let _ = session.publish_retained(&link.state(Instant::now()));
    link.refresh();
    let _ = session.publish_retained(&link.state(Instant::now()));
    while let Ok(r) = calls.recv() {
        let Some(call) = decode_link(&r) else {
            let _ = r.fail(ErrorCode::BadPayload);
            continue;
        };
        let reply = link.call(call, Instant::now());
        let _ = r.reply(&reply);
        let _ = session.publish_retained(&link.state(Instant::now()));
    }
}
