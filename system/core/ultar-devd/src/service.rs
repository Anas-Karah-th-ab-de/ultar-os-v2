//! The bus service `devices`: request dispatch for [`Daemon`].

use crate::daemon::Daemon;
use ultar_bus::messages::device_service::{
    ClearQuarantine, List, Rescan, Rollback, Shutdown, ShutdownReply, Switch, SERVICE,
};
use ultar_bus::{BusError, Client, ErrorCode, IncomingRequest, ServiceEndpoint};

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
    /// for good (`Err`). Blocks without polling.
    pub fn serve(&mut self, endpoint: &ServiceEndpoint) -> Result<(), BusError> {
        loop {
            let request = endpoint.recv()?;
            if self.handle(request) == Flow::Stop {
                return Ok(());
            }
        }
    }
}
