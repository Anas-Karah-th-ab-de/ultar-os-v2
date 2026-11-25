//! The bus side of the helper: `agent.ask`, and `agent.state` – where it thinks now.

use crate::activity::Log;
use crate::brain;
use crate::session;
use crate::toolbox::{BusThinker, BusTools};
use crate::tools::{self, Limits};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use ultar_bus::messages::agent::{
    AgentState, Answer, Area, Ask, ClearActivity, Cleared, Reach, ACT_KEY, MAX_REQUEST, SERVICE,
};
use ultar_bus::messages::llm::LlmState;
use ultar_bus::messages::llm_cloud::CloudState;
use ultar_bus::messages::mesh::MeshState;
use ultar_bus::messages::mesh_model::{Lender, Models};
use ultar_bus::messages::settings::{Changed, Get, SettingValue};
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_svckit::{Bus, Service};

/// How often the helper looks whether a paired device lends a model.
pub const TICK: Duration = Duration::from_secs(5);
const CALL_TIME: Duration = Duration::from_secs(2);

/// The helper service.
#[derive(Default)]
pub struct AgentService {
    local: LlmState,
    cloud: CloudState,
    lenders: Vec<Lender>,
    last: Option<AgentState>,
    next: u64,
    asking: BTreeMap<u64, (IncomingRequest, String)>,
    /// What the person closed.
    limits: Limits,
    reach: Option<Reach>,
    log: Log,
}

/// The setting that says how many days the list of what was done is kept.
pub const HISTORY_KEY: &str = "assistant.history_days";

fn now() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |d| d.as_secs())
}

impl AgentService {
    /// The helper; what it did is kept in `dir` (nowhere when `None`).
    pub fn new(dir: Option<&Path>) -> Self {
        Self {
            log: Log::open(dir),
            ..Self::default()
        }
    }

    fn publish_reach(&mut self, bus: &Bus<'_, Self>) {
        let now = tools::reach(&self.limits);
        if self.reach.as_ref() != Some(&now) {
            bus.retain(&now);
            self.reach = Some(now);
        }
    }

    /// One of the helper's settings has this value now.
    fn setting(&mut self, key: &str, value: &SettingValue, bus: &Bus<'_, Self>) {
        match (Area::of_setting(key), value) {
            (Some(area), SettingValue::Flag(allowed)) => {
                self.limits.off.retain(|a| *a != area);
                if !allowed {
                    self.limits.off.push(area);
                }
                self.publish_reach(bus);
            }
            (None, SettingValue::Flag(acts)) if key == ACT_KEY => {
                self.limits.read_only = !acts;
                self.publish_reach(bus);
            }
            (None, SettingValue::Integer(days)) if key == HISTORY_KEY => {
                self.log.keep_days(u64::try_from(*days).unwrap_or(0), now());
                bus.retain(self.log.list());
            }
            _ => {}
        }
    }

    /// Reads the helper's settings once at start (none without a settings service: then
    /// every area is open and the list is kept for the default time).
    fn read_settings(bus: &Bus<'_, Self>) {
        let _ = bus.later(|client| {
            let mut keys: Vec<String> = Area::ALL.into_iter().filter_map(Area::setting).collect();
            keys.extend([HISTORY_KEY.to_owned(), ACT_KEY.to_owned()]);
            let read = keys.into_iter().filter_map(|key| {
                let reply = client.call(&Get { key: key.clone() }, CALL_TIME);
                Some((key, reply.ok()?.value?))
            });
            let values: Vec<(String, SettingValue)> = read.collect();
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                for (key, value) in &values {
                    service.setting(key, value, bus);
                }
            })
        });
    }

    /// The areas in words, for the model.
    fn reach_words(&self) -> String {
        let ids = |open: bool| -> Vec<&str> {
            let areas = Area::ALL.into_iter();
            let kept = areas.filter(|a| self.limits.off.contains(a) != open);
            kept.map(Area::id).collect()
        };
        let acts = if self.limits.read_only {
            "you may only look, not act"
        } else {
            "you may act"
        };
        format!(
            "{acts}; open areas: {}; switched off by the person: {}",
            ids(true).join(", "),
            ids(false).join(", ")
        )
    }

    fn state(&self) -> AgentState {
        let (brain, lender) = brain::choose_with_cloud(&self.local, &self.lenders, &self.cloud);
        AgentState { brain, lender }
    }

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.state();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    /// Asks the mesh which paired devices lend a model (none when there is no mesh).
    fn look(&mut self, bus: &Bus<'_, Self>) {
        let _ = bus.later(|client| {
            let lenders = client.call(&Models, CALL_TIME).map(|m| m.lenders);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                service.lenders = lenders.unwrap_or_default();
                service.publish(bus);
            })
        });
    }

    fn ask(&mut self, request: IncomingRequest, text: String, bus: &Bus<'_, Self>) {
        self.next += 1;
        let id = self.next;
        let brain = self.state().brain;
        let limits = self.limits.clone();
        let (reach, lately) = (self.reach_words(), self.log.lately());
        let asked = text.clone();
        let started = bus.later(move |client| {
            let thinker = BusThinker {
                client,
                brain: brain.clone(),
            };
            let tools = BusTools {
                client,
                reach,
                lately,
            };
            let answer = session::answer(&text, brain, &thinker, &tools, &limits);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                service.reply(id, &answer, bus);
            })
        });
        if started {
            self.asking.insert(id, (request, asked));
        } else {
            let _ = request.fail(ErrorCode::ServiceBusy);
        }
    }

    fn reply(&mut self, id: u64, answer: &Answer, bus: &Bus<'_, Self>) {
        if let Some((request, asked)) = self.asking.remove(&id) {
            let _ = request.reply(answer);
            self.log.note(&asked, answer, now());
            bus.retain(self.log.list());
        }
    }
}

impl Service for AgentService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![
            LlmState::TOPIC,
            MeshState::TOPIC,
            CloudState::TOPIC,
            Changed::TOPIC,
        ]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        self.publish(bus);
        self.publish_reach(bus);
        bus.retain(self.log.list());
        Self::read_settings(bus);
        self.look(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        if request.method() == ClearActivity::METHOD {
            let ok = self.log.clear();
            bus.retain(self.log.list());
            let _ = request.reply(&Cleared { ok });
            return;
        }
        if request.method() != Ask::METHOD {
            let _ = request.fail(ErrorCode::UnknownMethod);
            return;
        }
        match request.decode::<Ask>() {
            Ok(ask) if !ask.text.trim().is_empty() && ask.text.len() <= MAX_REQUEST => {
                self.ask(request, ask.text, bus);
            }
            _ => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if event.topic == Changed::TOPIC {
            if let Ok(changed) = event.decode::<Changed>() {
                self.setting(&changed.key, &changed.value, bus);
            }
        } else if let Ok(state) = event.decode::<LlmState>() {
            self.local = state;
            self.publish(bus);
        } else if let Ok(state) = event.decode::<CloudState>() {
            self.cloud = state;
            self.publish(bus);
        } else {
            // The mesh changed: a lender may have come or gone.
            self.look(bus);
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.look(bus);
    }
}
