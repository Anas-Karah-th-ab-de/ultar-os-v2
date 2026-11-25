//! The ports over `ultar-bus`.
//!
//! Uses the launcher's `launcher.start` (version 2) and the drive service's `volumes.*`
//! contracts of `ultar_bus::messages` (the mirrored copy of the files branch is gone).

use super::{LauncherPort, Mounted, PortError, VolumesPort};
use std::time::Duration;
use ultar_bus::{BusError, Client, ErrorCode, TypedSubscription};

/// Timeout of every call.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// Store app (offers to install a missing app).
pub const STORE: &str = "org.ultar.store";

/// The launcher's and the drive service's own contracts.
pub mod contract {
    pub use ultar_bus::messages::launcher::{Resolve, ResolveReply, Role, Start, ERROR_UNKNOWN_APP};
    pub use ultar_bus::messages::volumes::{Eject, EjectOutcome, VolumesState};
}

/// The launcher's id of an app Files names `app`: base-system apps have one-part ids in
/// the launcher (`office`), Files names them `org.ultar.<id>`; a dotted store id stays.
fn launcher_id(app: &str) -> &str {
    app.strip_prefix("org.ultar.")
        .filter(|rest| !rest.is_empty() && !rest.contains('.'))
        .unwrap_or(app)
}

/// Files' name of an app the launcher resolves as `id` (the inverse of [`launcher_id`]).
fn shell_id(id: &str) -> String {
    if id.contains('.') {
        id.to_owned()
    } else {
        format!("org.ultar.{id}")
    }
}

fn port_error(e: &BusError) -> PortError {
    match e {
        BusError::Remote(ErrorCode::Service(contract::ERROR_UNKNOWN_APP)) => {
            PortError::NotInstalled
        }
        BusError::Remote(ErrorCode::Service(_)) => PortError::Refused,
        _ => PortError::Unavailable,
    }
}

/// The launcher over the bus.
pub struct BusLauncher(pub Client);

impl LauncherPort for BusLauncher {
    fn resolve(&mut self, uri: &str) -> Result<Option<String>, PortError> {
        let ask = contract::Resolve {
            uri: uri.to_owned(),
        };
        let reply = self.0.call(&ask, TIMEOUT).map_err(|e| port_error(&e))?;
        Ok(reply.app.map(|id| shell_id(&id)))
    }

    fn open(
        &mut self,
        app: &str,
        uri: &str,
        activation_token: Option<u64>,
    ) -> Result<(), PortError> {
        let start = contract::Start {
            app: launcher_id(app).to_owned(),
            role: contract::Role::Ui,
            activation_token,
            uri: Some(uri.to_owned()),
            new_window: false,
        };
        self.0
            .call(&start, TIMEOUT)
            .map(|_| ())
            .map_err(|e| port_error(&e))
    }
}

/// The drive service over the bus.
pub struct BusVolumes {
    bus: Client,
    state: Option<TypedSubscription<contract::VolumesState>>,
}

impl BusVolumes {
    /// Follows `volumes.state` (without it: no drives are shown).
    pub fn new(bus: Client) -> BusVolumes {
        let state = bus.subscribe().ok();
        BusVolumes { bus, state }
    }
}

impl VolumesPort for BusVolumes {
    fn eject(&mut self, id: &str) -> Result<bool, PortError> {
        let reply = self
            .bus
            .call(&contract::Eject { id: id.to_owned() }, TIMEOUT)
            .map_err(|e| port_error(&e))?;
        match reply.outcome {
            contract::EjectOutcome::Done => Ok(true),
            contract::EjectOutcome::Busy => Ok(false),
            contract::EjectOutcome::Unknown | contract::EjectOutcome::Failed => {
                Err(PortError::Refused)
            }
        }
    }

    fn changed(&mut self) -> Option<Vec<Mounted>> {
        let sub = self.state.as_ref()?;
        let mut last = None;
        while let Ok(Some(state)) = sub.try_recv() {
            last = Some(state);
        }
        last.map(|s| {
            s.mounted
                .into_iter()
                .map(|m| Mounted {
                    id: m.id,
                    name: m.name,
                    uri: m.uri,
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::contract::*;
    use ultar_bus::messages::launcher::{InstanceState, StartReply};
    use ultar_bus::messages::volumes::Mounted;
    use ultar_bus::{decode, encode, Call, Message};

    #[test]
    fn the_contracts_are_the_services_own() {
        let s = Start {
            app: "org.ultar.office".into(),
            role: Role::Ui,
            activation_token: Some(7),
            uri: Some("files://documents/a.docx".into()),
            new_window: false,
        };
        assert_eq!(<Start as Message>::VERSION, 2);
        assert_eq!((Start::SERVICE, Start::METHOD), ("launcher", "start"));
        assert_eq!(decode::<Start>(&encode(&s).unwrap()).unwrap(), s);
        let v = VolumesState {
            mounted: vec![Mounted {
                id: "sdb1".into(),
                name: "USB".into(),
                uri: "file:///m/USB".into(),
            }],
        };
        assert_eq!(decode::<VolumesState>(&encode(&v).unwrap()).unwrap(), v);
        let reply = decode::<StartReply>(&[1, 42, 0, 1]).unwrap();
        assert!(reply.opened && reply.state == InstanceState::Running && reply.pid == 42);
    }
}
