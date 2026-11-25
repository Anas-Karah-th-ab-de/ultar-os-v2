//! Contract of the power service: the current energy state of the device and the
//! actions of the power menu (sleep, restart, power off).
//!
//! Restart and power off go through the service manager (`init.power`): every service
//! ends in order first. Locking is `lock.lock` (lock screen), signing out
//! `session.sign_out` (session service).

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Where the device currently draws its energy from.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerSource {
    /// Running from the battery.
    Battery,
    /// Running from a charger or mains supply.
    External,
}

/// Performance profile in effect.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PowerProfile {
    /// Highest performance.
    Performance,
    /// Balance between performance and consumption.
    #[default]
    Balanced,
    /// Lowest consumption.
    Saver,
}

/// Thermal situation of the device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub enum ThermalLevel {
    /// No throttling.
    #[default]
    Normal,
    /// Getting warm; background work is slowed down.
    Warm,
    /// Hot; performance is reduced.
    Hot,
    /// Critical; the device may shut down.
    Critical,
}

/// `power.state` – the current energy state (retained).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerState {
    /// Current energy source.
    pub source: PowerSource,
    /// Battery charge in percent, `None` on devices without battery.
    pub battery_percent: Option<u8>,
    /// Whether the battery is being charged.
    pub charging: bool,
    /// Profile in effect.
    pub profile: PowerProfile,
    /// Thermal situation.
    pub thermal: ThermalLevel,
}

impl Message for PowerState {
    const VERSION: u16 = 1;
}

impl Topic for PowerState {
    const TOPIC: &'static str = "power.state";
    const RETAINED: bool = true;
}

/// Name of the power service on the bus.
pub const SERVICE: &str = "power";

/// `power.actions` – which actions of the power menu this device offers (retained).
/// A menu entry whose action is `false` is shown disabled with the reason "not
/// supported on this device".
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PowerActions {
    /// Sleep (suspend to memory) is offered by the kernel.
    pub suspend: bool,
    /// Restart is possible (the service manager is reachable).
    pub restart: bool,
    /// Power off is possible (the service manager is reachable).
    pub power_off: bool,
    /// Restart and power off ask for an administrator's password because other
    /// people are signed in too.
    pub needs_admin: bool,
}

/// `power.suspend` – put the device to sleep.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Suspend {}

/// `power.restart` – end every service in order and restart.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Restart {}

/// `power.off` – end every service in order and power off.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PowerOff {}

/// How a power action went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionOutcome {
    /// Begun (restart/power off) or done (sleep, answered after waking up).
    Done,
    /// This device cannot do it (see [`PowerActions`]).
    Unsupported,
    /// An administrator's password was needed and the dialog was cancelled.
    Cancelled,
    /// The caller may not do it.
    Refused,
    /// Another action is running.
    Busy,
    /// The kernel or the service manager failed.
    Failed,
}

/// Answer to [`Suspend`], [`Restart`] and [`PowerOff`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActionReply {
    /// The result.
    pub outcome: ActionOutcome,
}

/// `power.torch` – switch the camera light (`Some`) or only ask (`None`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Torch {
    /// On, off, or `None` to ask.
    pub on: Option<bool>,
}

/// Answer to [`Torch`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TorchReply {
    /// The device has a torch.
    pub present: bool,
    /// It is on.
    pub on: bool,
}

impl Call for Torch {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "torch";
    type Reply = TorchReply;
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    PowerActions,
    Suspend,
    Restart,
    PowerOff,
    ActionReply,
    Torch,
    TorchReply
);

impl Topic for PowerActions {
    const TOPIC: &'static str = "power.actions";
    const RETAINED: bool = true;
}

impl Call for Suspend {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "suspend";
    type Reply = ActionReply;
}

impl Call for Restart {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "restart";
    type Reply = ActionReply;
}

impl Call for PowerOff {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "off";
    type Reply = ActionReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn power_state_round_trips() {
        let phone = PowerState {
            source: PowerSource::Battery,
            battery_percent: Some(81),
            charging: false,
            profile: PowerProfile::Saver,
            thermal: ThermalLevel::Warm,
        };
        let desktop = PowerState {
            source: PowerSource::External,
            battery_percent: None,
            charging: false,
            profile: PowerProfile::default(),
            thermal: ThermalLevel::default(),
        };
        for state in [phone, desktop] {
            assert_eq!(
                decode::<PowerState>(&encode(&state).unwrap()).unwrap(),
                state
            );
        }
        assert!(ThermalLevel::Critical > ThermalLevel::Hot);
        let actions = PowerActions {
            suspend: false,
            restart: true,
            power_off: true,
            needs_admin: true,
        };
        assert_eq!(
            decode::<PowerActions>(&encode(&actions).unwrap()).unwrap(),
            actions
        );
        for outcome in [
            ActionOutcome::Done,
            ActionOutcome::Unsupported,
            ActionOutcome::Cancelled,
            ActionOutcome::Refused,
            ActionOutcome::Busy,
            ActionOutcome::Failed,
        ] {
            let r = ActionReply { outcome };
            assert_eq!(decode::<ActionReply>(&encode(&r).unwrap()).unwrap(), r);
        }
        assert_eq!(PowerProfile::default(), PowerProfile::Balanced);
    }
}
