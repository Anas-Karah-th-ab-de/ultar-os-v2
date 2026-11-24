//! What the service's logic needs from the outside, as traits (so the
//! logic runs in tests without a bus or a compositor), and the bus
//! implementation of the settings part.

use std::time::Duration;
use ultar_bus::messages::display_service::Outputs;
use ultar_bus::messages::settings::{
    Confirm, DisplayDescriptor, ListRequest, Revert, Set, SetDisplays, SettingValue,
};
use ultar_bus::{BusError, Client, ErrorCode};
use ultar_surface::proto::Request;

/// How long a call to the settings service may take.
pub const SETTINGS_TIMEOUT: Duration = Duration::from_secs(3);

/// Why a port call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortError {
    /// The other side is not there (not running, not connected, no answer).
    Unavailable,
    /// The other side refused (its error code; 0 when it gave none).
    Refused(u32),
}

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortError::Unavailable => f.write_str("not available"),
            PortError::Refused(code) => write!(f, "refused ({code})"),
        }
    }
}

impl std::error::Error for PortError {}

impl From<BusError> for PortError {
    fn from(e: BusError) -> Self {
        match e.code() {
            ErrorCode::Service(code) => PortError::Refused(code),
            ErrorCode::PermissionDenied
            | ErrorCode::BadPayload
            | ErrorCode::VersionMismatch
            | ErrorCode::UnknownMethod => PortError::Refused(0),
            _ => PortError::Unavailable,
        }
    }
}

/// One display setting as `settings.list` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The key.
    pub key: String,
    /// The value in effect.
    pub value: SettingValue,
    /// Waits for "Keep?".
    pub pending: bool,
}

/// The settings service.
pub trait SettingsPort {
    /// Reports the connected displays; returns their ids in the same order.
    fn set_displays(&self, displays: &[DisplayDescriptor]) -> Result<Vec<String>, PortError>;
    /// Every setting of the display panel.
    fn display_items(&self) -> Result<Vec<Item>, PortError>;
    /// Sets one setting.
    fn set(&self, key: &str, value: SettingValue) -> Result<(), PortError>;
    /// Keeps a waiting change.
    fn confirm(&self, key: &str) -> Result<u32, PortError>;
    /// Takes a waiting change back.
    fn revert(&self, key: &str) -> Result<u32, PortError>;
}

/// The compositor connection.
pub trait CompositorPort {
    /// Sends a request (false: not connected).
    fn send(&self, request: Request) -> bool;
}

/// Where `display.outputs` goes.
pub trait Publisher {
    /// Publishes (retained).
    fn outputs(&self, outputs: &Outputs);
}

/// The settings service over the bus.
#[derive(Clone)]
pub struct BusSettings {
    client: Client,
}

impl BusSettings {
    /// Calls through `client`.
    pub fn new(client: Client) -> Self {
        BusSettings { client }
    }
}

impl SettingsPort for BusSettings {
    fn set_displays(&self, displays: &[DisplayDescriptor]) -> Result<Vec<String>, PortError> {
        let call = SetDisplays {
            displays: displays.to_vec(),
        };
        let reply = self.client.call(&call, SETTINGS_TIMEOUT)?;
        if reply.ids.len() != displays.len() {
            return Err(PortError::Refused(0));
        }
        Ok(reply.ids)
    }

    fn display_items(&self) -> Result<Vec<Item>, PortError> {
        let call = ListRequest {
            panel: Some("display".to_owned()),
        };
        let reply = self.client.call(&call, SETTINGS_TIMEOUT)?;
        Ok(reply
            .panels
            .into_iter()
            .flat_map(|p| p.items)
            .filter(|i| !i.template)
            .map(|i| Item {
                key: i.key,
                value: i.value,
                pending: i.pending,
            })
            .collect())
    }

    fn set(&self, key: &str, value: SettingValue) -> Result<(), PortError> {
        let call = Set {
            key: key.to_owned(),
            value,
        };
        self.client.call(&call, SETTINGS_TIMEOUT)?;
        Ok(())
    }

    fn confirm(&self, key: &str) -> Result<u32, PortError> {
        let call = Confirm {
            key: Some(key.to_owned()),
        };
        Ok(self.client.call(&call, SETTINGS_TIMEOUT)?.resolved)
    }

    fn revert(&self, key: &str) -> Result<u32, PortError> {
        let call = Revert {
            key: Some(key.to_owned()),
        };
        Ok(self.client.call(&call, SETTINGS_TIMEOUT)?.resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_errors_map_to_port_errors() {
        assert_eq!(PortError::from(BusError::Timeout), PortError::Unavailable);
        assert_eq!(
            PortError::from(BusError::Remote(ErrorCode::Service(7))),
            PortError::Refused(7)
        );
        assert_eq!(
            PortError::from(BusError::Remote(ErrorCode::PermissionDenied)),
            PortError::Refused(0)
        );
        assert_eq!(
            PortError::from(BusError::Remote(ErrorCode::UnknownService)),
            PortError::Unavailable
        );
        assert!(!PortError::Refused(3).to_string().is_empty());
    }
}
