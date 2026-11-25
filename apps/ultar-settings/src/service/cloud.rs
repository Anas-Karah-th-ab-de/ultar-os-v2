//! The cloud model behind the section on Settings › Assistant, as a port: over the bus
//! ([`BusCloud`]) and for tests ([`FakeCloud`]). The key goes in; nothing gives it back.

use super::bus::{call, Retained};
use super::fake::Shared;
use super::{PortError, PortResult};
use std::time::Duration;
use ultar_bus::messages::llm_cloud::{ClearKey, CloudState, Saved, SetKey};
use ultar_bus::{Client, Text};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// What the section asks of the cloud-model service.
pub trait CloudPort {
    /// Whether a key is stored, and for which provider.
    fn state(&mut self) -> PortResult<CloudState>;
    /// Stores the person's key.
    fn set_key(&mut self, key: &str) -> PortResult<()>;
    /// Forgets the key.
    fn clear_key(&mut self) -> PortResult<()>;
}

/// `llmcloud` over the bus.
pub struct BusCloud {
    client: Client,
    state: Retained<CloudState>,
}

impl std::fmt::Debug for BusCloud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusCloud")
    }
}

impl BusCloud {
    /// Subscribes to `llmcloud.state`.
    pub fn new(client: &Client) -> BusCloud {
        BusCloud {
            client: client.clone(),
            state: Retained::new(client),
        }
    }
}

fn saved(reply: PortResult<Saved>) -> PortResult<()> {
    match reply {
        Ok(Saved { ok: true }) => Ok(()),
        Ok(_) => Err(PortError::Refused(Text::key(
            "app.settings.cloud.error.key",
        ))),
        Err(e) => Err(e),
    }
}

impl CloudPort for BusCloud {
    fn state(&mut self) -> PortResult<CloudState> {
        self.state.get()
    }

    fn set_key(&mut self, key: &str) -> PortResult<()> {
        let key = key.to_owned();
        saved(call(&self.client, &SetKey { key }, CALL_TIMEOUT))
    }

    fn clear_key(&mut self) -> PortResult<()> {
        saved(call(&self.client, &ClearKey {}, CALL_TIMEOUT))
    }
}

/// The cloud model of the test world (`World::system.cloud`). It keeps no key: only that
/// one was given.
#[derive(Debug, Clone)]
pub struct FakeCloud(pub Shared);

impl CloudPort for FakeCloud {
    fn state(&mut self) -> PortResult<CloudState> {
        self.0
            .borrow()
            .system
            .cloud
            .clone()
            .ok_or(PortError::Unavailable)
    }

    fn set_key(&mut self, key: &str) -> PortResult<()> {
        let mut world = self.0.borrow_mut();
        // the log says that a key came, never which
        world
            .log
            .push(format!("llmcloud.set_key ({} characters)", key.len()));
        if key.contains(' ') {
            return Err(PortError::Refused(Text::key(
                "app.settings.cloud.error.key",
            )));
        }
        let cloud = world.system.cloud.as_mut().ok_or(PortError::Unavailable)?;
        cloud.configured = true;
        Ok(())
    }

    fn clear_key(&mut self) -> PortResult<()> {
        let mut world = self.0.borrow_mut();
        world.log.push("llmcloud.clear_key".into());
        let cloud = world.system.cloud.as_mut().ok_or(PortError::Unavailable)?;
        cloud.configured = false;
        Ok(())
    }
}
