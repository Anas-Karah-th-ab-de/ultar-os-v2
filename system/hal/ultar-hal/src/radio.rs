//! Radio class: cellular modem, SIM, signal and network registration.

use crate::error::HalResult;

/// State of the modem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModemState {
    /// No modem in this device.
    Absent,
    /// Present, radio switched off (flight mode).
    Off,
    /// Starting up or loading firmware.
    Initializing,
    /// Running.
    Ready,
    /// The modem stopped responding.
    Failed,
}

/// State of a SIM slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimState {
    /// No card.
    Absent,
    /// Card waits for the PIN.
    PinRequired,
    /// Card is blocked and waits for the PUK.
    PukRequired,
    /// Card is usable.
    Ready,
    /// Card cannot be read.
    Error,
}

/// Radio access technology.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccessTechnology {
    /// 2G.
    Gsm,
    /// 3G.
    Umts,
    /// 4G.
    Lte,
    /// 5G.
    Nr,
    /// Not known.
    Unknown,
}

/// Strength of the cellular signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignalStrength {
    /// Received power in dBm.
    pub dbm: i16,
    /// Technology the value was measured on.
    pub technology: AccessTechnology,
}

impl SignalStrength {
    /// Signal bars from 0 (no service) to 4 (excellent), using the usual thresholds
    /// of the measured technology.
    pub const fn bars(&self) -> u8 {
        let thresholds: [i16; 4] = match self.technology {
            AccessTechnology::Lte | AccessTechnology::Nr => [-120, -110, -100, -90],
            _ => [-107, -101, -93, -85],
        };
        let mut bars = 0;
        while bars < 4 && self.dbm >= thresholds[bars as usize] {
            bars += 1;
        }
        bars
    }
}

/// Registration with a network.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegistrationState {
    /// Not registered and not searching.
    NotRegistered,
    /// Searching for a network.
    Searching,
    /// Registered in the home network.
    Home,
    /// Registered in a foreign network.
    Roaming,
    /// The network refused the registration.
    Denied,
    /// Only emergency calls are possible.
    EmergencyOnly,
}

impl RegistrationState {
    /// True if normal service is available.
    pub const fn is_registered(self) -> bool {
        matches!(self, RegistrationState::Home | RegistrationState::Roaming)
    }
}

/// Current network registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    /// State of the registration.
    pub state: RegistrationState,
    /// Network code (country code followed by network code), if registered.
    pub operator_code: Option<String>,
    /// Network name as the network announces it, if any.
    pub operator_name: Option<String>,
    /// Technology in use.
    pub technology: AccessTechnology,
}

/// Cellular modem of a backend.
pub trait Radio: Send + Sync {
    /// State of the modem.
    fn modem_state(&self) -> HalResult<ModemState>;
    /// Switches the radio on or off.
    fn set_powered(&self, powered: bool) -> HalResult<()>;
    /// Number of SIM slots.
    fn sim_slots(&self) -> HalResult<u8>;
    /// State of one SIM slot (counted from zero).
    fn sim_state(&self, slot: u8) -> HalResult<SimState>;
    /// Signal strength; `None` without service.
    fn signal(&self) -> HalResult<Option<SignalStrength>>;
    /// Network registration.
    fn registration(&self) -> HalResult<Registration>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_bars() {
        let lte = |dbm| SignalStrength {
            dbm,
            technology: AccessTechnology::Lte,
        };
        assert_eq!(lte(-130).bars(), 0);
        assert_eq!(lte(-120).bars(), 1);
        assert_eq!(lte(-105).bars(), 2);
        assert_eq!(lte(-95).bars(), 3);
        assert_eq!(lte(-90).bars(), 4);
        assert_eq!(lte(-40).bars(), 4);
        let gsm = |dbm| SignalStrength {
            dbm,
            technology: AccessTechnology::Gsm,
        };
        assert_eq!(gsm(-110).bars(), 0);
        assert_eq!(gsm(-100).bars(), 2);
        assert_eq!(gsm(-85).bars(), 4);
    }

    #[test]
    fn registered_states() {
        assert!(RegistrationState::Home.is_registered());
        assert!(RegistrationState::Roaming.is_registered());
        for state in [
            RegistrationState::NotRegistered,
            RegistrationState::Searching,
            RegistrationState::Denied,
            RegistrationState::EmergencyOnly,
        ] {
            assert!(!state.is_registered());
        }
    }
}
