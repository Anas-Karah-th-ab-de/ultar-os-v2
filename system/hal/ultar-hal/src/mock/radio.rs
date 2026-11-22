//! Mock cellular modem and satellite receiver.

use super::cell::MockCell;
use crate::error::{HalError, HalResult};
use crate::radio::{
    AccessTechnology, ModemState, Radio, Registration, RegistrationState, SignalStrength, SimState,
};
use crate::sensors::{Gnss, GnssFix, SatelliteInfo};

#[derive(Debug)]
struct Modem {
    state: ModemState,
    sims: Vec<SimState>,
    signal: Option<SignalStrength>,
    registration: Registration,
}

fn unregistered() -> Registration {
    Registration {
        state: RegistrationState::NotRegistered,
        operator_code: None,
        operator_name: None,
        technology: AccessTechnology::Unknown,
    }
}

/// Scriptable modem. Starts ready with one usable SIM, registered at home on 4G.
#[derive(Debug)]
pub struct MockRadio {
    cell: MockCell<Modem>,
}

impl Default for MockRadio {
    fn default() -> Self {
        MockRadio {
            cell: MockCell::new(Modem {
                state: ModemState::Ready,
                sims: vec![SimState::Ready],
                signal: Some(SignalStrength {
                    dbm: -95,
                    technology: AccessTechnology::Lte,
                }),
                registration: Registration {
                    state: RegistrationState::Home,
                    operator_code: Some("00101".to_string()),
                    operator_name: Some("Mock Net".to_string()),
                    technology: AccessTechnology::Lte,
                },
            }),
        }
    }
}

impl MockRadio {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Sets the modem state directly (for example [`ModemState::Absent`] or `Failed`).
    pub fn set_modem_state(&self, state: ModemState) {
        self.cell.state().state = state;
    }

    /// Sets the state of every SIM slot; the length is the number of slots.
    pub fn set_sims(&self, sims: &[SimState]) {
        self.cell.state().sims = sims.to_vec();
    }

    /// Sets the signal; `None` means no service.
    pub fn set_signal(&self, signal: Option<SignalStrength>) {
        self.cell.state().signal = signal;
    }

    /// Sets the network registration.
    pub fn set_registration(&self, registration: Registration) {
        self.cell.state().registration = registration;
    }
}

impl Radio for MockRadio {
    fn modem_state(&self) -> HalResult<ModemState> {
        self.cell.call(|modem| Ok(modem.state))
    }

    fn set_powered(&self, powered: bool) -> HalResult<()> {
        self.cell.call(|modem| match modem.state {
            ModemState::Absent => Err(HalError::NotSupported),
            ModemState::Failed => Err(HalError::Disconnected),
            _ => {
                modem.state = if powered {
                    ModemState::Ready
                } else {
                    ModemState::Off
                };
                Ok(())
            }
        })
    }

    fn sim_slots(&self) -> HalResult<u8> {
        self.cell
            .call(|modem| Ok(u8::try_from(modem.sims.len()).unwrap_or(u8::MAX)))
    }

    fn sim_state(&self, slot: u8) -> HalResult<SimState> {
        self.cell.call(|modem| {
            modem
                .sims
                .get(usize::from(slot))
                .copied()
                .ok_or(HalError::NotFound)
        })
    }

    /// No signal while the radio is not ready.
    fn signal(&self) -> HalResult<Option<SignalStrength>> {
        self.cell.call(|modem| {
            Ok(match modem.state {
                ModemState::Ready => modem.signal,
                _ => None,
            })
        })
    }

    /// Not registered while the radio is not ready.
    fn registration(&self) -> HalResult<Registration> {
        self.cell.call(|modem| {
            Ok(match modem.state {
                ModemState::Ready => modem.registration.clone(),
                _ => unregistered(),
            })
        })
    }
}

#[derive(Debug, Default)]
struct Receiver {
    running: bool,
    fix: Option<GnssFix>,
    satellites: Vec<SatelliteInfo>,
}

/// Scriptable satellite receiver. Starts switched off without a fix.
#[derive(Debug, Default)]
pub struct MockGnss {
    cell: MockCell<Receiver>,
}

impl MockGnss {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Sets the position the receiver reports while running.
    pub fn set_fix(&self, fix: Option<GnssFix>) {
        self.cell.state().fix = fix;
    }

    /// Sets the satellites in view.
    pub fn set_satellites(&self, satellites: &[SatelliteInfo]) {
        self.cell.state().satellites = satellites.to_vec();
    }
}

impl Gnss for MockGnss {
    fn start(&self) -> HalResult<()> {
        self.cell.call(|receiver| {
            receiver.running = true;
            Ok(())
        })
    }

    fn stop(&self) -> HalResult<()> {
        self.cell.call(|receiver| {
            receiver.running = false;
            Ok(())
        })
    }

    fn is_running(&self) -> HalResult<bool> {
        self.cell.call(|receiver| Ok(receiver.running))
    }

    /// No fix while the receiver is off.
    fn fix(&self) -> HalResult<Option<GnssFix>> {
        self.cell
            .call(|receiver| Ok(receiver.fix.filter(|_| receiver.running)))
    }

    /// No satellites while the receiver is off.
    fn satellites(&self) -> HalResult<Vec<SatelliteInfo>> {
        self.cell.call(|receiver| {
            Ok(if receiver.running {
                receiver.satellites.clone()
            } else {
                Vec::new()
            })
        })
    }
}
