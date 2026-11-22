//! Calls of the bus service `devices` offered by the device daemon (`ultar-devd`)
//! for the settings panel "Drivers & hardware" and the `udevctl` tool.
//!
//! Every call answers with a typed reply; a request the daemon understood but
//! refused carries a [`Refusal`] instead of a transport error, so the panel can
//! show the reason through its i18n key.

use crate::message::{Call, Message, Text};
use serde::{Deserialize, Serialize};

/// Name of the bus service.
pub const SERVICE: &str = "devices";

/// Phase of one device as shown in the panel.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevicePhase {
    /// Seen, driver search not finished.
    Detected,
    /// A driver is being tried.
    Probing,
    /// A driver is active and the device works.
    Active,
    /// No driver package fits the device.
    NoDriver,
    /// Driver packages fit, but none could be used.
    Failed,
}

/// A driver package (active one or alternative).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct DriverInfo {
    /// Driver identifier from the package manifest.
    pub id: String,
    /// Driver version.
    pub version: String,
    /// Functional class name (`net`, `storage`, …).
    pub class: String,
    /// i18n key of the driver's display name.
    pub name_key: String,
    /// Quarantine reason code (`load.init-failed`, …) when quarantined for the device.
    pub quarantined: Option<String>,
}

/// One row of the device list.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct DeviceInfo {
    /// Address of the device on this machine (`pci:00:1f.6`).
    pub address: String,
    /// Primary hardware identifier (`pci:8086:0d4c`).
    pub hardware_id: String,
    /// Technical label from the inventory, if any (data, not translated).
    pub label: Option<String>,
    /// Current phase.
    pub phase: DevicePhase,
    /// The active driver, if any.
    pub active: Option<DriverInfo>,
    /// Other fitting drivers, best first.
    pub alternatives: Vec<DriverInfo>,
    /// `id version` of the driver a roll-back would return to.
    pub previous: Option<String>,
    /// Capability identifiers the active driver provides.
    pub capabilities: Vec<String>,
    /// Why the device has no working driver (i18n key), `None` when active.
    pub reason: Option<Text>,
}

/// Why the daemon refused an operation.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefusalKind {
    /// The running kernel offers no interface for this operation.
    Unsupported,
    /// No device at this address.
    UnknownDevice,
    /// The driver does not fit the device or is not in the depot.
    NotACandidate,
    /// The driver is quarantined for the device.
    Quarantined,
    /// The driver is already the active one.
    AlreadyActive,
    /// There is no earlier driver to return to.
    NothingToRollBack,
    /// Not possible in the device's current phase.
    WrongPhase,
    /// The request itself is malformed (e.g. unparsable version).
    BadRequest,
    /// The hardware or the depot could not be read.
    SystemUnreadable,
    /// An internal inconsistency.
    Internal,
}

/// A refused operation with a displayable reason.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// Machine-readable kind.
    pub kind: RefusalKind,
    /// i18n key and arguments for the user.
    pub reason: Text,
}

/// Result of an operation on one device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Done; the device row after the operation.
    Done(Box<DeviceInfo>),
    /// Refused.
    Refused(Refusal),
}

/// `devices.list` – all present devices.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct List {}

/// Answer to [`List`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ListReply {
    /// Devices ordered by address.
    pub devices: Vec<DeviceInfo>,
    /// Notices about the daemon itself (state file recovered, incomplete inventory, …).
    pub notices: Vec<Text>,
    /// Whether this daemon can switch between drivers that need different kernel drivers.
    pub kernel_switch_supported: bool,
}

/// `devices.switch` – use another driver for a device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Switch {
    /// Address of the device.
    pub address: String,
    /// Identifier of the wanted driver.
    pub driver: String,
    /// Version of the wanted driver.
    pub version: String,
}

/// `devices.rollback` – return to the driver used before the last switch.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Rollback {
    /// Address of the device.
    pub address: String,
}

/// Answer to [`Switch`] and [`Rollback`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct OperationReply {
    /// What happened.
    pub outcome: Outcome,
    /// `true` when the wanted driver failed and the earlier one was restored.
    pub restored: bool,
}

/// `devices.clear_quarantine` – release quarantined drivers.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ClearQuarantine {
    /// Only this device; `None` for all devices.
    pub address: Option<String>,
    /// Only this driver (`id`, `version`) on `address`; needs `address`.
    pub driver: Option<(String, String)>,
}

/// Answer to [`ClearQuarantine`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ClearQuarantineReply {
    /// Number of released entries, or the refusal.
    pub released: Result<u32, Refusal>,
}

/// `devices.rescan` – read the hardware and the depot again.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Rescan {
    /// Only search again for this driverless device; `None` re-reads everything.
    pub address: Option<String>,
}

/// Answer to [`Rescan`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct RescanReply {
    /// Devices after the rescan, or the refusal.
    pub devices: Result<Vec<DeviceInfo>, Refusal>,
    /// Addresses that appeared.
    pub added: Vec<String>,
    /// Addresses that disappeared.
    pub removed: Vec<String>,
}

/// `devices.shutdown` – ask the daemon to end cleanly.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Shutdown {}

/// Answer to [`Shutdown`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ShutdownReply {}

macro_rules! contract {
    ($($call:ident => $method:literal, $reply:ident;)*) => {$(
        impl Message for $call {
            const VERSION: u16 = 1;
        }
        impl Message for $reply {
            const VERSION: u16 = 1;
        }
        impl Call for $call {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $method;
            type Reply = $reply;
        }
    )*};
}

contract! {
    List => "list", ListReply;
    Switch => "switch", OperationReply;
    ClearQuarantine => "clear_quarantine", ClearQuarantineReply;
    Rescan => "rescan", RescanReply;
    Shutdown => "shutdown", ShutdownReply;
}

impl Message for Rollback {
    const VERSION: u16 = 1;
}

impl Call for Rollback {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "rollback";
    type Reply = OperationReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};
    use crate::topic::{is_valid_name, is_valid_segment_name};

    fn row() -> DeviceInfo {
        DeviceInfo {
            address: "pci:00:1f.6".into(),
            hardware_id: "pci:8086:0d4c".into(),
            label: Some("intel-i219-lm".into()),
            phase: DevicePhase::Active,
            active: Some(DriverInfo {
                id: "gen-intel-eth".into(),
                version: "1.0.0".into(),
                class: "net".into(),
                name_key: "driver.gen-intel-eth.name".into(),
                quarantined: None,
            }),
            alternatives: Vec::new(),
            previous: None,
            capabilities: vec!["net".into()],
            reason: None,
        }
    }

    #[test]
    fn replies_round_trip() {
        let list = ListReply {
            devices: vec![row()],
            notices: vec![Text::key("devd.notice.state-recovered")],
            kernel_switch_supported: false,
        };
        assert_eq!(decode::<ListReply>(&encode(&list).unwrap()).unwrap(), list);
        let refused = OperationReply {
            outcome: Outcome::Refused(Refusal {
                kind: RefusalKind::Unsupported,
                reason: Text::key("devd.error.unsupported"),
            }),
            restored: false,
        };
        assert_eq!(
            decode::<OperationReply>(&encode(&refused).unwrap()).unwrap(),
            refused
        );
        let done = OperationReply {
            outcome: Outcome::Done(Box::new(row())),
            restored: true,
        };
        assert_eq!(
            decode::<OperationReply>(&encode(&done).unwrap()).unwrap(),
            done
        );
        let rescan = RescanReply {
            devices: Ok(vec![row()]),
            added: vec!["usb:0-3".into()],
            removed: Vec::new(),
        };
        assert_eq!(
            decode::<RescanReply>(&encode(&rescan).unwrap()).unwrap(),
            rescan
        );
        let cleared = ClearQuarantineReply { released: Ok(2) };
        assert_eq!(
            decode::<ClearQuarantineReply>(&encode(&cleared).unwrap()).unwrap(),
            cleared
        );
    }

    fn check<C: Call>(method: &str) {
        assert_eq!(C::SERVICE, SERVICE);
        assert_eq!(C::METHOD, method);
        assert!(is_valid_name(C::SERVICE) && is_valid_segment_name(C::METHOD));
    }

    #[test]
    fn method_names() {
        check::<List>("list");
        check::<Switch>("switch");
        check::<Rollback>("rollback");
        check::<ClearQuarantine>("clear_quarantine");
        check::<Rescan>("rescan");
        check::<Shutdown>("shutdown");
    }
}
