//! The service for apps from other systems behind Settings › Apps from other systems
//! (`docs/FREMD-APPS.md`, contract `ultar_bus::messages::foreign`), as a port: over the bus
//! ([`BusForeign`]) and for tests ([`FakeForeign`]).

use super::bus::call;
use super::fake::Shared;
use super::{PortError, PortResult};
use std::time::Duration;
use ultar_bus::messages::foreign::{
    Changed, Choices, ForeignApp, InstallOutcome, List, Outcome, Progress, Remove,
    RuntimeState, RuntimeStateReply, RuntimeUpdate, SetChoices,
};
use ultar_bus::{Client, Text, TypedSubscription};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// What the page asks of the service.
pub trait ForeignPort {
    /// The Linux area.
    fn runtime(&mut self) -> PortResult<RuntimeStateReply>;
    /// The apps from other systems.
    fn list(&mut self) -> PortResult<Vec<ForeignApp>>;
    /// What an app may reach from its next start (more is confirmed by an administrator:
    /// [`Outcome::Asking`]).
    fn set_choices(&mut self, id: &str, choices: Choices) -> PortResult<Outcome>;
    /// Removes an app (and its data unless kept).
    fn remove(&mut self, id: &str, keep_data: bool) -> PortResult<Outcome>;
    /// Updates the Linux area from its sources (a job).
    fn update_runtime(&mut self) -> PortResult<InstallOutcome>;
    /// Job steps since the last call, and whether the list changed.
    fn news(&mut self) -> (Vec<Progress>, bool);
}

/// `foreign` over the bus.
pub struct BusForeign {
    client: Client,
    progress: Option<TypedSubscription<Progress>>,
    changed: Option<TypedSubscription<Changed>>,
}

impl std::fmt::Debug for BusForeign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusForeign")
    }
}

impl BusForeign {
    /// Follows `foreign.progress` and `foreign.changed`.
    pub fn new(client: &Client) -> BusForeign {
        BusForeign {
            client: client.clone(),
            progress: client.subscribe().ok(),
            changed: client.subscribe().ok(),
        }
    }
}

impl ForeignPort for BusForeign {
    fn runtime(&mut self) -> PortResult<RuntimeStateReply> {
        call(&self.client, &RuntimeState {}, CALL_TIMEOUT)
    }

    fn list(&mut self) -> PortResult<Vec<ForeignApp>> {
        call(&self.client, &List {}, CALL_TIMEOUT).map(|r| r.apps)
    }

    fn set_choices(&mut self, id: &str, choices: Choices) -> PortResult<Outcome> {
        let ask = SetChoices {
            id: id.to_owned(),
            choices,
        };
        call(&self.client, &ask, CALL_TIMEOUT).map(|r| r.outcome)
    }

    fn remove(&mut self, id: &str, keep_data: bool) -> PortResult<Outcome> {
        let ask = Remove {
            id: id.to_owned(),
            keep_data,
        };
        // removal purges the package in the installation cell
        call(&self.client, &ask, 12 * CALL_TIMEOUT).map(|r| r.outcome)
    }

    fn update_runtime(&mut self) -> PortResult<InstallOutcome> {
        call(&self.client, &RuntimeUpdate {}, CALL_TIMEOUT).map(|r| r.outcome)
    }

    fn news(&mut self) -> (Vec<Progress>, bool) {
        let mut steps = Vec::new();
        if let Some(sub) = &self.progress {
            while let Ok(Some(p)) = sub.try_recv() {
                steps.push(p);
            }
        }
        let mut changed = false;
        if let Some(sub) = &self.changed {
            while let Ok(Some(_)) = sub.try_recv() {
                changed = true;
            }
        }
        (steps, changed)
    }
}

/// What the fake service holds (`World::system.foreign`).
#[derive(Debug, Clone, Default)]
pub struct ForeignWorld {
    /// The Linux area.
    pub runtime: RuntimeStateReply,
    /// The apps.
    pub apps: Vec<ForeignApp>,
    /// What `set_choices` answers when it grants more (`None`: done at once).
    pub asking: Option<u64>,
    /// Job steps still to deliver.
    pub steps: Vec<Progress>,
    /// The list changed since the last look.
    pub changed: bool,
}

/// The service of the test world.
#[derive(Debug, Clone)]
pub struct FakeForeign(pub Shared);

impl FakeForeign {
    fn with<R>(&mut self, log: String, f: impl FnOnce(&mut ForeignWorld) -> R) -> PortResult<R> {
        let mut world = self.0.borrow_mut();
        world.log.push(log);
        if let Some(reason) = world.refuse.take() {
            return Err(PortError::Refused(reason));
        }
        world
            .system
            .foreign
            .as_mut()
            .map(f)
            .ok_or(PortError::Unavailable)
    }
}

impl ForeignPort for FakeForeign {
    fn runtime(&mut self) -> PortResult<RuntimeStateReply> {
        let world = self.0.borrow();
        let f = world.system.foreign.as_ref().ok_or(PortError::Unavailable)?;
        Ok(f.runtime.clone())
    }

    fn list(&mut self) -> PortResult<Vec<ForeignApp>> {
        let world = self.0.borrow();
        let f = world.system.foreign.as_ref().ok_or(PortError::Unavailable)?;
        Ok(f.apps.clone())
    }

    fn set_choices(&mut self, id: &str, choices: Choices) -> PortResult<Outcome> {
        let log = format!("foreign.set_choices {id} {choices:?}");
        self.with(log, |f| {
            let Some(app) = f.apps.iter_mut().find(|a| a.id == id) else {
                return Outcome::Unknown;
            };
            match f.asking {
                Some(job) if choices.widens(&app.choices) => Outcome::Asking(job),
                _ => {
                    app.choices = choices;
                    Outcome::Done
                }
            }
        })
    }

    fn remove(&mut self, id: &str, keep_data: bool) -> PortResult<Outcome> {
        self.with(format!("foreign.remove {id} keep={keep_data}"), |f| {
            let before = f.apps.len();
            f.apps.retain(|a| a.id != id);
            if f.apps.len() < before {
                Outcome::Done
            } else {
                Outcome::Unknown
            }
        })
    }

    fn update_runtime(&mut self) -> PortResult<InstallOutcome> {
        self.with("foreign.runtime_update".into(), |f| {
            if f.runtime.installed {
                InstallOutcome::Started(9)
            } else {
                InstallOutcome::RuntimeMissing
            }
        })
    }

    fn news(&mut self) -> (Vec<Progress>, bool) {
        let mut world = self.0.borrow_mut();
        match world.system.foreign.as_mut() {
            Some(f) => (std::mem::take(&mut f.steps), std::mem::take(&mut f.changed)),
            None => (Vec::new(), false),
        }
    }
}

/// i18n text of an outcome that did not do what was asked.
pub fn outcome_error(outcome: Outcome) -> Option<Text> {
    let key = match outcome {
        Outcome::Done | Outcome::Asking(_) => return None,
        Outcome::Unknown => "app.settings.foreign.error.unknown",
        Outcome::Busy => "app.settings.foreign.error.busy",
        Outcome::Invalid | Outcome::Failed => "app.settings.error.failed",
    };
    Some(Text::key(key))
}
