//! The cloud model behind the section on Settings › Assistant, as a port: over the bus
//! ([`BusCloud`]) and for tests ([`FakeCloud`]). The key goes in; nothing gives it back.

use super::bus::{call, Retained};
use super::fake::Shared;
use super::{PortError, PortResult};
use std::time::Duration;
use ultar_bus::messages::agent::{AgentState, Brain};
use ultar_bus::messages::llm_cloud::{
    ClearKey, CloudState, Configure, ModelInfo, Refusal, Saved, SetKey,
};
use ultar_bus::{Client, Text};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Storing a key asks the provider first.
const KEY_TIMEOUT: Duration = Duration::from_secs(25);

/// What the section asks of the cloud-model service.
pub trait CloudPort {
    /// Provider, model and whether a key is stored.
    fn state(&mut self) -> PortResult<CloudState>;
    /// Stores the person's key (the provider is asked first).
    fn set_key(&mut self, key: &str) -> PortResult<()>;
    /// Forgets the key.
    fn clear_key(&mut self) -> PortResult<()>;
    /// Chooses provider, address and model.
    fn configure(&mut self, choice: Configure) -> PortResult<()>;
    /// What the helper thinks with right now (`None`: no helper on this device).
    fn brain(&mut self) -> Option<Brain>;
}

/// `llmcloud` over the bus.
pub struct BusCloud {
    client: Client,
    state: Retained<CloudState>,
    helper: Retained<AgentState>,
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
            helper: Retained::new(client),
        }
    }
}

/// The message for something that was not saved.
pub fn refusal_key(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::KeyRefused => "app.settings.cloud.error.refused",
        Refusal::Storage => "app.settings.error.failed",
        Refusal::Invalid | Refusal::None => "app.settings.cloud.error.invalid",
    }
}

fn saved(reply: PortResult<Saved>) -> PortResult<()> {
    match reply {
        Ok(Saved { ok: true, .. }) => Ok(()),
        Ok(not) => Err(PortError::Refused(Text::key(refusal_key(not.refusal)))),
        Err(e) => Err(e),
    }
}

impl CloudPort for BusCloud {
    fn state(&mut self) -> PortResult<CloudState> {
        self.state.get()
    }

    fn set_key(&mut self, key: &str) -> PortResult<()> {
        let key = key.to_owned();
        saved(call(&self.client, &SetKey { key }, KEY_TIMEOUT))
    }

    fn clear_key(&mut self) -> PortResult<()> {
        saved(call(&self.client, &ClearKey {}, CALL_TIMEOUT))
    }

    fn configure(&mut self, choice: Configure) -> PortResult<()> {
        saved(call(&self.client, &choice, CALL_TIMEOUT))
    }

    fn brain(&mut self) -> Option<Brain> {
        self.helper.get().ok().map(|state| state.brain)
    }
}

/// The key the fake provider refuses.
pub const REFUSED_KEY: &str = "sk-refused";

/// The cloud model of the test world (`World::system.cloud`). It keeps no key: only that
/// one was given.
#[derive(Debug, Clone)]
pub struct FakeCloud(pub Shared);

impl FakeCloud {
    fn refuse(refusal: Refusal) -> PortResult<()> {
        Err(PortError::Refused(Text::key(refusal_key(refusal))))
    }
}

impl CloudPort for FakeCloud {
    fn state(&mut self) -> PortResult<CloudState> {
        let world = self.0.borrow();
        world.system.cloud.clone().ok_or(PortError::Unavailable)
    }

    fn set_key(&mut self, key: &str) -> PortResult<()> {
        let mut world = self.0.borrow_mut();
        // the log says that a key came, never which
        world
            .log
            .push(format!("llmcloud.set_key ({} characters)", key.len()));
        if key.contains(' ') {
            return Self::refuse(Refusal::Invalid);
        }
        if key == REFUSED_KEY {
            return Self::refuse(Refusal::KeyRefused);
        }
        let cloud = world.system.cloud.as_mut().ok_or(PortError::Unavailable)?;
        cloud.configured = true;
        cloud.verified = true;
        let model = |id: &str| ModelInfo {
            id: id.to_owned(),
            name: id.to_owned(),
        };
        cloud.models = vec![model("model-big"), model("model-small")];
        cloud.model = "model-small".to_owned();
        Ok(())
    }

    fn clear_key(&mut self) -> PortResult<()> {
        let mut world = self.0.borrow_mut();
        world.log.push("llmcloud.clear_key".into());
        let cloud = world.system.cloud.as_mut().ok_or(PortError::Unavailable)?;
        cloud.configured = false;
        cloud.verified = false;
        cloud.models.clear();
        Ok(())
    }

    fn configure(&mut self, choice: Configure) -> PortResult<()> {
        let mut world = self.0.borrow_mut();
        let said = (&choice.provider, &choice.endpoint, &choice.model);
        world.log.push(format!("llmcloud.configure {said:?}"));
        let cloud = world.system.cloud.as_mut().ok_or(PortError::Unavailable)?;
        let known = cloud.providers.iter().find(|p| p.id == choice.provider);
        let Some(needs) = known.map(|p| p.needs_endpoint) else {
            return Self::refuse(Refusal::Invalid);
        };
        if needs && !choice.endpoint.starts_with("https://") {
            return Self::refuse(Refusal::Invalid);
        }
        if cloud.provider != choice.provider || cloud.endpoint != choice.endpoint {
            cloud.configured = false;
            cloud.verified = false;
            cloud.models.clear();
        }
        cloud.provider = choice.provider;
        cloud.endpoint = choice.endpoint;
        cloud.model = choice.model;
        Ok(())
    }

    /// As the helper would choose without a model on the person's devices.
    fn brain(&mut self) -> Option<Brain> {
        let world = self.0.borrow();
        let cloud = world.system.cloud.as_ref()?;
        Some(if cloud.configured {
            Brain::Cloud
        } else {
            Brain::Minimal
        })
    }
}
