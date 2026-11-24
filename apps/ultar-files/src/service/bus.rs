//! The ports over `ultar-bus`.
//!
//! The launcher's `start` (version 2) and the drive service's `volumes.*` contracts are not on
//! this base's `ultar-bus` yet (they come with the app launcher and the system services
//! branches). [`contract`] mirrors their wire layout field by field (postcard, same order,
//! same versions); on integration `contract` is replaced by `ultar_bus::messages::{launcher,
//! volumes}` without other changes.

use super::{LauncherPort, Mounted, PortError, VolumesPort};
use std::time::Duration;
use ultar_bus::{BusError, Client, ErrorCode, TypedSubscription};

/// Timeout of every call.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// Store app (offers to install a missing app).
pub const STORE: &str = "org.ultar.store";

/// Mirror of the launcher and drive contracts (see the module docs).
pub mod contract {
    use serde::{Deserialize, Serialize};
    use ultar_bus::{Call, Message, Topic};

    /// `launcher::Role`.
    #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Role {
        /// The app's window.
        Ui,
        /// Its background service.
        Service,
    }

    /// `launcher::InstanceState`.
    #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
    pub enum InstanceState {
        /// Starting.
        Starting,
        /// Running.
        Running,
        /// Stopping.
        Stopping,
        /// Waiting to restart.
        Backoff,
        /// Stopped.
        Stopped,
        /// Crashed.
        Crashed,
        /// Failed.
        Failed,
    }

    /// `launcher.start` v2.
    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
    pub struct Start {
        /// App id.
        pub app: String,
        /// Which program.
        pub role: Role,
        /// Activation token.
        pub activation_token: Option<u64>,
        /// What to open.
        pub uri: Option<String>,
        /// Another window.
        pub new_window: bool,
    }

    /// Answer to [`Start`].
    #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
    pub struct StartReply {
        /// State now.
        pub state: InstanceState,
        /// Process id.
        pub pid: u32,
        /// It ran already.
        pub already: bool,
        /// The URI was handed over.
        pub opened: bool,
    }

    /// `volumes::Mounted`.
    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
    pub struct Mounted {
        /// Id.
        pub id: String,
        /// Label.
        pub name: String,
        /// URI.
        pub uri: String,
    }

    /// `volumes.state` (retained).
    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
    pub struct VolumesState {
        /// Mounted drives.
        pub mounted: Vec<Mounted>,
    }

    /// `volumes.eject`.
    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
    pub struct Eject {
        /// Drive id.
        pub id: String,
    }

    /// `volumes::EjectOutcome`.
    #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
    pub enum EjectOutcome {
        /// Ejected.
        Done,
        /// In use.
        Busy,
        /// No such drive.
        Unknown,
        /// Failed.
        Failed,
    }

    /// Answer to [`Eject`].
    #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
    pub struct EjectReply {
        /// What happened.
        pub outcome: EjectOutcome,
    }

    /// Error code of the launcher for an unknown app.
    pub const ERROR_UNKNOWN_APP: u32 = 1;

    impl Message for Start {
        const VERSION: u16 = 2;
    }
    impl Message for StartReply {
        const VERSION: u16 = 2;
    }
    impl Message for VolumesState {
        const VERSION: u16 = 1;
    }
    impl Message for Eject {
        const VERSION: u16 = 1;
    }
    impl Message for EjectReply {
        const VERSION: u16 = 1;
    }
    impl Call for Start {
        const SERVICE: &'static str = "launcher";
        const METHOD: &'static str = "start";
        type Reply = StartReply;
    }
    impl Call for Eject {
        const SERVICE: &'static str = "volumes";
        const METHOD: &'static str = "eject";
        type Reply = EjectReply;
    }
    impl Topic for VolumesState {
        const TOPIC: &'static str = "volumes.state";
        const RETAINED: bool = true;
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
    fn open(
        &mut self,
        app: &str,
        uri: &str,
        activation_token: Option<u64>,
    ) -> Result<(), PortError> {
        let start = contract::Start {
            app: app.to_owned(),
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
    use ultar_bus::{decode, encode};

    #[test]
    fn mirrored_contracts_round_trip() {
        let s = Start {
            app: "org.ultar.office".into(),
            role: Role::Ui,
            activation_token: Some(7),
            uri: Some("files://documents/a.docx".into()),
            new_window: false,
        };
        assert_eq!(decode::<Start>(&encode(&s).unwrap()).unwrap(), s);
        let v = VolumesState {
            mounted: vec![Mounted {
                id: "sdb1".into(),
                name: "USB".into(),
                uri: "file:///m/USB".into(),
            }],
        };
        assert_eq!(decode::<VolumesState>(&encode(&v).unwrap()).unwrap(), v);
        // the layout the launcher answers with: state Running (1), pid, already, opened
        let reply = decode::<StartReply>(&[1, 42, 0, 1]).unwrap();
        assert!(reply.opened && reply.state == InstanceState::Running && reply.pid == 42);
    }
}
