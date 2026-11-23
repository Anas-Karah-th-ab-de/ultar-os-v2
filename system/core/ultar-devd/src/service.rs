//! The bus service `devices`: request dispatch for [`Daemon`].

use crate::access::ADMIN_METHODS;
use crate::daemon::Daemon;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ultar_bus::messages::device_service::{
    ClearQuarantine, List, Rescan, Rollback, Shutdown, ShutdownReply, Switch, SERVICE,
};
use ultar_bus::{BusError, Client, ErrorCode, IncomingRequest, ServiceEndpoint};
use ultar_service::log::{Level, Record};

/// Reads the kernel's device events (netlink, `ultar_sys::uevent`) on a thread of its own and raises
/// the returned flag for each; `None` when the kernel refuses the socket (then the event counter
/// file `/sys/kernel/uevent_seqnum` is read instead).
fn watch_kernel_events() -> Option<Arc<AtomicBool>> {
    let socket = ultar_sys::uevent::UeventSocket::open().ok()?;
    let flag = Arc::new(AtomicBool::new(false));
    let raised = flag.clone();
    std::thread::Builder::new()
        .name("uevents".into())
        .spawn(move || {
            while let Ok(event) = socket.next(None) {
                if event.is_some() {
                    raised.store(true, Ordering::SeqCst);
                }
            }
        })
        .ok()?;
    Some(flag)
}

/// How often the daemon looks for kernel device events between requests (the bus endpoint and the
/// event socket cannot be waited on together).
pub const KERNEL_EVENT_POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// Whether the daemon keeps serving after a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Wait for the next request.
    Continue,
    /// `devices.shutdown` was answered: stop.
    Stop,
}

impl Daemon {
    /// Registers the service `devices` on the bus.
    pub fn register(bus: &Client) -> Result<ServiceEndpoint, BusError> {
        bus.register(SERVICE)
    }

    /// Answers one request. A reply that cannot be sent (caller gone) is not an error of
    /// the daemon; the state change it reports has happened and is published.
    pub fn handle(&mut self, request: IncomingRequest) -> Flow {
        let mut flow = Flow::Continue;
        let method = request.method().to_string();
        if let Some(operation) = ADMIN_METHODS.iter().find(|name| **name == method) {
            if !self.access.is_admin(request.caller()) {
                // Refused before the request is even decoded: a caller
                // without the permission learns nothing about the state.
                self.logger.emit(
                    Record::new(Level::Warn, "devd.admin_refused")
                        .word("op", operation)
                        .id_or_invalid("peer", request.caller()),
                );
                let _ = request.fail(ErrorCode::PermissionDenied);
                return flow;
            }
        }
        let _sent = match method.as_str() {
            "list" => request.respond::<List>(|_| Ok(self.list())),
            "switch" => request.respond::<Switch>(|call| {
                Ok(self.switch(&call.address, &call.driver, &call.version))
            }),
            "rollback" => request.respond::<Rollback>(|call| Ok(self.rollback(&call.address))),
            "clear_quarantine" => {
                request.respond::<ClearQuarantine>(|call| Ok(self.clear_quarantine(call)))
            }
            "rescan" => request.respond::<Rescan>(|call| Ok(self.rescan(call.address.as_deref()))),
            // Only a well-formed request stops the daemon.
            "shutdown" => request.respond::<Shutdown>(|_| {
                flow = Flow::Stop;
                Ok(ShutdownReply {})
            }),
            _ => request.fail(ErrorCode::UnknownMethod),
        };
        flow
    }

    /// Serves requests until `devices.shutdown` arrives (`Ok`) or the bus connection ends
    /// for good (`Err`). Between requests it looks at the kernel's device event counter
    /// (every [`KERNEL_EVENT_POLL`]) and reads the hardware again when a device came, went
    /// or changed its binding.
    pub fn serve(&mut self, endpoint: &ServiceEndpoint) -> Result<(), BusError> {
        let events = watch_kernel_events();
        loop {
            match endpoint.recv_timeout(KERNEL_EVENT_POLL) {
                Ok(request) => {
                    if self.handle(request) == Flow::Stop {
                        return Ok(());
                    }
                    self.note_kernel_events();
                }
                Err(BusError::Timeout) => {
                    // the event socket says it without a file read; without it the counter file
                    let changed = match &events {
                        Some(flag) => flag.swap(false, Ordering::SeqCst),
                        None => self.kernel_events_pending(),
                    };
                    if changed {
                        let _ = self.rescan(None);
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}
