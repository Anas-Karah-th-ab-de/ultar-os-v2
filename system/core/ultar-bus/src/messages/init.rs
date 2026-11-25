//! Contract of the service manager `ultar-init`: what runs, and starting,
//! stopping and restarting services.
//!
//! Both calls are for administrators: the policy gives them to the
//! administrator's identity, and `ultar-init` checks the caller against the
//! pseudo method [`ADMIN_METHOD`] of its own service as well (a policy that
//! opens `init.status` to an app does not open the control call).

use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "init";
/// Pseudo method whose call right marks an administrator of `init`.
pub const ADMIN_METHOD: &str = "admin";
/// Pseudo method whose call right lets an identity end the machine with
/// [`Power`] (the power service; a rule listing only `init.power` is not enough).
pub const SHUTDOWN_METHOD: &str = "shutdown";
/// Most services one reply lists.
pub const MAX_SERVICES: usize = 256;
/// Method of [`Switch`].
pub const SWITCH_METHOD: &str = "switch";

/// Where a service is in its life.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServiceState {
    /// Waits for the services it is ordered after.
    Waiting,
    /// Started; its readiness has not arrived yet.
    Starting,
    /// Running (and ready, if it reports readiness).
    Running,
    /// A stop was sent; waiting for the process to end.
    Stopping,
    /// Ended; waits before the next start.
    Backoff,
    /// A one-shot service that ended (see `last_exit`).
    Done,
    /// Stopped on request, or never started (manual services).
    Stopped,
    /// Given up: ended too often in a short time, or cannot be started.
    Failed,
}

impl ServiceState {
    /// Stable lower-case word (for tools and records).
    pub const fn word(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Backoff => "backoff",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// How a process ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    /// It returned this exit code.
    Code(i32),
    /// A signal with this number ended it.
    Signal(i32),
}

/// One service in a [`StatusReply`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ServiceStatus {
    /// Name in the service files.
    pub name: String,
    /// Current state.
    pub state: ServiceState,
    /// Process id while it runs (0 otherwise).
    pub pid: u32,
    /// User id it runs as.
    pub uid: u32,
    /// Starts after the first one (crashes and requested restarts).
    pub restarts: u32,
    /// How the previous run ended, if there was one.
    pub last_exit: Option<Exit>,
}

/// `init.status` – the state of every service.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Status {}

/// Answer to [`Status`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct StatusReply {
    /// The services, by name.
    pub services: Vec<ServiceStatus>,
}

/// What [`Control`] asks for.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Start a stopped or failed service.
    Start,
    /// Stop a service (it stays stopped until started again).
    Stop,
    /// Stop and start again.
    Restart,
}

impl Action {
    /// Stable lower-case word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

/// `init.control` – start, stop or restart one service. Answered when the
/// request is accepted (the service may still be starting or stopping).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Control {
    /// What to do.
    pub action: Action,
    /// The service.
    pub service: String,
}

/// Answer to [`Control`]: the state right after the request.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ControlReply {
    /// State of the service now.
    pub state: ServiceState,
}

/// How the machine should end (`init.power`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerAction {
    /// Ordered shutdown, then power off.
    PowerOff,
    /// Ordered shutdown, then restart.
    Reboot,
}

/// `init.power` – end every service in order, then power off or restart. Only
/// the power service calls it (policy), after its own rules; administrators'
/// tools may too (`init.*`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Power {
    /// Power off or restart.
    pub action: PowerAction,
}

/// Answer to [`Power`]: the shutdown has begun.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PowerReply {}

/// Error code of `init.control` for a service name that is not known.
pub const ERROR_UNKNOWN_SERVICE: u32 = 1;
/// Error code of `init.control`: the service is already in the state.
pub const ERROR_ALREADY: u32 = 2;
/// Error code of `init.control`: the system is shutting down.
pub const ERROR_SHUTTING_DOWN: u32 = 3;
/// Service error of [`Switch`]: the service was switched a moment ago.
pub const ERROR_TOO_SOON: u32 = 4;

impl Message for Status {
    const VERSION: u16 = 1;
}
impl Message for StatusReply {
    const VERSION: u16 = 1;
}
impl Message for Control {
    const VERSION: u16 = 1;
}
impl Message for ControlReply {
    const VERSION: u16 = 1;
}

impl Call for Status {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "status";
    type Reply = StatusReply;
}

impl Message for Power {
    const VERSION: u16 = 1;
}
impl Message for PowerReply {
    const VERSION: u16 = 1;
}

impl Call for Power {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "power";
    type Reply = PowerReply;
}

impl Call for Control {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "control";
    type Reply = ControlReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode_versioned, encode};

    #[test]
    fn messages_round_trip_and_words_are_distinct() {
        let reply = StatusReply {
            services: vec![ServiceStatus {
                name: "logd".into(),
                state: ServiceState::Running,
                pid: 12,
                uid: 802,
                restarts: 1,
                last_exit: Some(Exit::Signal(9)),
            }],
        };
        let back: StatusReply =
            decode_versioned(StatusReply::VERSION, &encode(&reply).unwrap()).unwrap();
        assert_eq!(back, reply);
        let control = Control {
            action: Action::Restart,
            service: "bus".into(),
        };
        let back: Control = decode_versioned(Control::VERSION, &encode(&control).unwrap()).unwrap();
        assert_eq!(back, control);
        let states = [
            ServiceState::Waiting,
            ServiceState::Starting,
            ServiceState::Running,
            ServiceState::Stopping,
            ServiceState::Backoff,
            ServiceState::Done,
            ServiceState::Stopped,
            ServiceState::Failed,
        ];
        let words: std::collections::BTreeSet<&str> = states.iter().map(|s| s.word()).collect();
        assert_eq!(words.len(), states.len());
        assert_ne!(Action::Stop.word(), Action::Start.word());
    }
}

/// `init.switch` – switch one service on or off for the person. Not an administrator
/// call and no way to become one: a service file names the one system service that may
/// switch it (`switch = "system.x"`), and such a file is only accepted for a service that
/// starts on request, runs as its own user and holds no kernel right. Today: the
/// language model, switched by `ultar-llm-bus` following the person's setting.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Switch {
    /// The service.
    pub service: String,
    /// On or off.
    pub on: bool,
}

impl Message for Switch {
    const VERSION: u16 = 1;
}

impl Call for Switch {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = SWITCH_METHOD;
    type Reply = ControlReply;
}
