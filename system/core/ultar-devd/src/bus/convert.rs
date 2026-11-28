//! Manager views and errors as contract types of the bus service `devices`.

use crate::builtin::{Builtin, LoadDetail};
use crate::error::DevdError;
use crate::fsm::Phase;
use crate::ops::DeviceView;
use crate::types::{DriverKey, FailureReason};
use ultar_bus::messages::device_service::{
    DeviceInfo, DevicePhase, DriverInfo, Refusal, RefusalKind,
};
use ultar_bus::messages::devices::DriverPhase;
use ultar_bus::{Text, TextArg};
use ultar_driverpkg::Candidate;
use ultar_hal_system::BindState;

/// The panel phase of a manager phase (`Removed` devices are not listed; shown as detected).
pub fn device_phase(phase: Phase) -> DevicePhase {
    match phase {
        Phase::Detected | Phase::Removed => DevicePhase::Detected,
        Phase::Probing => DevicePhase::Probing,
        Phase::Active => DevicePhase::Active,
        Phase::NoDriver => DevicePhase::NoDriver,
        Phase::Failed => DevicePhase::Failed,
    }
}

/// The `devices.driver_state` phase of a device without working driver.
pub fn unserved_phase(phase: Phase) -> DriverPhase {
    match phase {
        Phase::NoDriver => DriverPhase::Missing,
        _ => DriverPhase::Failed,
    }
}

fn driver_info(candidate: &Candidate, quarantined: Option<FailureReason>) -> DriverInfo {
    DriverInfo {
        id: candidate.id.clone(),
        version: candidate.version.to_string(),
        class: candidate.class.as_str().to_string(),
        name_key: candidate.name_key.clone(),
        quarantined: quarantined.map(FailureReason::code),
    }
}

fn verbatim(key: &str, text: &str) -> Text {
    Text::key(key).with(TextArg::Verbatim(text.to_string()))
}

/// The reason one candidate could not be used, from the loader's detail.
fn detail_reason(detail: &LoadDetail) -> Option<Text> {
    Some(match detail {
        LoadDetail::NoActivationRecord => Text::key("devd.reason.not-loadable"),
        LoadDetail::NotBound { kernel_driver, .. } => {
            verbatim("devd.reason.not-bound", kernel_driver)
        }
        LoadDetail::PackageMissing => Text::key("devd.reason.package-missing"),
        LoadDetail::PackageRejected(code) => {
            Text::key("devd.reason.rejected-detail").with(TextArg::Key((*code).to_string()))
        }
        LoadDetail::WrongPackage => Text::key("devd.reason.rejected-detail")
            .with(TextArg::Key("devd.detail.wrong-package".to_string())),
        LoadDetail::BadActivationRecord => Text::key("devd.reason.rejected-detail").with(
            TextArg::Key("devd.detail.bad-activation-record".to_string()),
        ),
        LoadDetail::ProbeFailed(error) => verbatim("devd.reason.probe-failed", error),
        LoadDetail::DeviceGone | LoadDetail::Loaded { .. } => return None,
    })
}

/// The kernel driver that runs a device the depot has no package for (bridges, the SMBus
/// controller, …): such a device works, it only cannot be switched to another driver.
pub fn kernel_driver(view: &DeviceView, builtin: &Builtin) -> Option<String> {
    if view.phase != Phase::NoDriver {
        return None;
    }
    match builtin.binding(&view.device.address) {
        BindState::Bound { driver } => Some(driver),
        _ => None,
    }
}

/// `true` when the device works: with a driver package or with the kernel's own driver.
pub fn served(view: &DeviceView, builtin: &Builtin) -> bool {
    view.phase == Phase::Active || kernel_driver(view, builtin).is_some()
}

/// Why a device has no working driver, or the note that the kernel's own driver runs it;
/// `None` for a device active with a package.
pub fn reason(view: &DeviceView, builtin: &Builtin) -> Option<Text> {
    match view.phase {
        Phase::Active => return None,
        Phase::NoDriver => {
            return Some(match kernel_driver(view, builtin) {
                Some(driver) => verbatim("devd.reason.kernel-only", &driver),
                None => Text::key("devd.reason.no-package"),
            })
        }
        _ => {}
    }
    for alternative in &view.alternatives {
        let key = DriverKey::of(&alternative.driver);
        if let Some(text) = builtin
            .detail(&view.device.address, &key)
            .as_ref()
            .and_then(detail_reason)
        {
            return Some(text);
        }
        if let Some(code) = alternative.quarantined {
            return Some(Text::key(&format!("devd.reason.{}", code.code())));
        }
    }
    Some(Text::key("devd.reason.unknown"))
}

/// One row of the device list.
pub fn device_info(view: &DeviceView, builtin: &Builtin) -> DeviceInfo {
    DeviceInfo {
        address: view.device.address.clone(),
        hardware_id: view.device.primary.to_string(),
        label: view.device.label.clone(),
        phase: if kernel_driver(view, builtin).is_some() {
            DevicePhase::Active
        } else {
            device_phase(view.phase)
        },
        active: view.active.as_ref().map(|c| driver_info(c, None)),
        alternatives: view
            .alternatives
            .iter()
            .map(|a| driver_info(&a.driver, a.quarantined))
            .collect(),
        previous: view.previous.as_ref().map(DriverKey::to_string),
        capabilities: view.capabilities.iter().map(|c| c.name.clone()).collect(),
        reason: reason(view, builtin),
    }
}

/// A refusal with the given kind and message key.
pub fn refuse(kind: RefusalKind, key: &str) -> Refusal {
    Refusal {
        kind,
        reason: Text::key(key),
    }
}

/// The refusal for a manager error. The text carries the device address and, where it
/// applies, the driver as arguments `{a}` and `{b}`.
pub fn refusal(error: &DevdError) -> Refusal {
    let kind = match error {
        DevdError::UnknownDevice(_) => RefusalKind::UnknownDevice,
        DevdError::NotACandidate { .. } => RefusalKind::NotACandidate,
        DevdError::Quarantined { .. } => RefusalKind::Quarantined,
        DevdError::AlreadyActive(_) => RefusalKind::AlreadyActive,
        DevdError::NothingToRollBack(_) => RefusalKind::NothingToRollBack,
        DevdError::WrongPhase { .. } => RefusalKind::WrongPhase,
        DevdError::Unsupported { .. } => RefusalKind::Unsupported,
        DevdError::Transition(_) => RefusalKind::Internal,
    };
    let mut reason = Text::key(error.code());
    match error {
        DevdError::UnknownDevice(device) | DevdError::NothingToRollBack(device) => {
            reason = reason.with(TextArg::Verbatim(device.clone()));
        }
        DevdError::NotACandidate { device, driver }
        | DevdError::Quarantined { device, driver }
        | DevdError::Unsupported { device, driver } => {
            reason = reason
                .with(TextArg::Verbatim(device.clone()))
                .with(TextArg::Verbatim(driver.to_string()));
        }
        DevdError::AlreadyActive(driver) => {
            reason = reason.with(TextArg::Verbatim(driver.to_string()));
        }
        DevdError::WrongPhase { device, phase } => {
            reason = reason
                .with(TextArg::Verbatim(device.clone()))
                .with(TextArg::Key(format!("devd.phase.{}", phase.as_str())));
        }
        DevdError::Transition(_) => {}
    }
    Refusal { kind, reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm::{InvalidTransition, Signal};
    use ultar_driverpkg::Version;

    #[test]
    fn phases() {
        assert_eq!(device_phase(Phase::Active), DevicePhase::Active);
        assert_eq!(device_phase(Phase::Probing), DevicePhase::Probing);
        assert_eq!(device_phase(Phase::NoDriver), DevicePhase::NoDriver);
        assert_eq!(device_phase(Phase::Failed), DevicePhase::Failed);
        assert_eq!(device_phase(Phase::Detected), DevicePhase::Detected);
        assert_eq!(device_phase(Phase::Removed), DevicePhase::Detected);
        assert_eq!(unserved_phase(Phase::NoDriver), DriverPhase::Missing);
        assert_eq!(unserved_phase(Phase::Failed), DriverPhase::Failed);
    }

    fn check(error: DevdError, kind: RefusalKind, args: Vec<TextArg>) {
        let refusal = refusal(&error);
        assert_eq!(refusal.kind, kind);
        assert_eq!(refusal.reason.key, error.code());
        assert_eq!(refusal.reason.args, args);
    }

    #[test]
    fn refusals_about_a_device_and_a_driver() {
        let driver = DriverKey::new("nvme", Version::new(1, 0, 0));
        let device = "pci:01:00.0".to_string();
        let both = vec![
            TextArg::Verbatim(device.clone()),
            TextArg::Verbatim("nvme 1.0.0".into()),
        ];
        let pair = |device: &String, driver: &DriverKey| (device.clone(), driver.clone());
        let (d, k) = pair(&device, &driver);
        check(
            DevdError::Unsupported {
                device: d,
                driver: k,
            },
            RefusalKind::Unsupported,
            both.clone(),
        );
        let (d, k) = pair(&device, &driver);
        check(
            DevdError::NotACandidate {
                device: d,
                driver: k,
            },
            RefusalKind::NotACandidate,
            both.clone(),
        );
        let (d, k) = pair(&device, &driver);
        check(
            DevdError::Quarantined {
                device: d,
                driver: k,
            },
            RefusalKind::Quarantined,
            both,
        );
        check(
            DevdError::AlreadyActive(driver),
            RefusalKind::AlreadyActive,
            vec![TextArg::Verbatim("nvme 1.0.0".into())],
        );
    }

    #[test]
    fn refusals_about_a_device_only() {
        let device = "pci:01:00.0".to_string();
        let one = vec![TextArg::Verbatim(device.clone())];
        check(
            DevdError::UnknownDevice(device.clone()),
            RefusalKind::UnknownDevice,
            one.clone(),
        );
        check(
            DevdError::NothingToRollBack(device.clone()),
            RefusalKind::NothingToRollBack,
            one,
        );
        check(
            DevdError::WrongPhase {
                device: device.clone(),
                phase: Phase::Active,
            },
            RefusalKind::WrongPhase,
            vec![
                TextArg::Verbatim(device),
                TextArg::Key("devd.phase.active".into()),
            ],
        );
        let transition = InvalidTransition {
            from: Phase::Removed,
            signal: Signal::Rescan,
        };
        check(
            DevdError::Transition(transition),
            RefusalKind::Internal,
            Vec::new(),
        );
        let plain = refuse(RefusalKind::BadRequest, "devd.error.bad-request");
        assert_eq!(plain.reason, Text::key("devd.error.bad-request"));
    }

    #[test]
    fn detail_reasons() {
        let not_bound = LoadDetail::NotBound {
            kernel_driver: "gen-hda".into(),
            bound_to: None,
        };
        assert_eq!(
            detail_reason(&not_bound),
            Some(verbatim("devd.reason.not-bound", "gen-hda"))
        );
        assert_eq!(
            detail_reason(&LoadDetail::NoActivationRecord).map(|t| t.key),
            Some("devd.reason.not-loadable".into())
        );
        assert_eq!(
            detail_reason(&LoadDetail::PackageMissing).map(|t| t.key),
            Some("devd.reason.package-missing".into())
        );
        for detail in [
            LoadDetail::PackageRejected("driverpkg.signature.invalid"),
            LoadDetail::WrongPackage,
            LoadDetail::BadActivationRecord,
        ] {
            let text = detail_reason(&detail).unwrap();
            assert_eq!(text.key, "devd.reason.rejected-detail");
            assert!(matches!(&text.args[0], TextArg::Key(_)));
        }
        assert_eq!(
            detail_reason(&LoadDetail::ProbeFailed("BAR0 fehlt".into())),
            Some(verbatim("devd.reason.probe-failed", "BAR0 fehlt"))
        );
        assert_eq!(detail_reason(&LoadDetail::DeviceGone), None);
        let loaded = LoadDetail::Loaded {
            kernel_driver: "nvme".into(),
        };
        assert_eq!(detail_reason(&loaded), None);
    }
}
