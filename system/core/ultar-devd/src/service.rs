//! The bus service `devices`: request dispatch for [`Daemon`].

use crate::access::ADMIN_METHODS;
use crate::daemon::Daemon;
use ultar_bus::messages::device_service::{
    ClearQuarantine, List, Rescan, Rollback, Shutdown, ShutdownReply, Switch, SERVICE,
};
use ultar_bus::{BusError, Client, ErrorCode, IncomingRequest, ServiceEndpoint};
use ultar_service::log::{Level, Record};

/// How often the daemon looks at the kernel's device event counter between requests.
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
        loop {
            match endpoint.recv_timeout(KERNEL_EVENT_POLL) {
                Ok(request) => {
                    if self.handle(request) == Flow::Stop {
                        return Ok(());
                    }
                    self.note_kernel_events();
                }
                Err(BusError::Timeout) => {
                    if self.kernel_events_pending() {
                        let _ = self.rescan(None);
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}
