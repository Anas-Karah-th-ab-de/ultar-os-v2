//! The bus side of the helper: `agent.ask`, and `agent.state` – where it thinks now.

use crate::brain;
use crate::session;
use crate::toolbox::{BusThinker, BusTools};
use std::collections::BTreeMap;
use std::time::Duration;
use ultar_bus::messages::agent::{AgentState, Answer, Ask, MAX_REQUEST, SERVICE};
use ultar_bus::messages::llm::LlmState;
use ultar_bus::messages::mesh::MeshState;
use ultar_bus::messages::mesh_model::{Lender, Models};
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_svckit::{Bus, Service};

/// How often the helper looks whether a paired device lends a model.
pub const TICK: Duration = Duration::from_secs(5);
const CALL_TIME: Duration = Duration::from_secs(2);

/// The helper service.
#[derive(Default)]
pub struct AgentService {
    local: LlmState,
    lenders: Vec<Lender>,
    last: Option<AgentState>,
    next: u64,
    asking: BTreeMap<u64, IncomingRequest>,
}

impl AgentService {
    fn state(&self) -> AgentState {
        let (brain, lender) = brain::choose(&self.local, &self.lenders);
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
        let started = bus.later(move |client| {
            let thinker = BusThinker {
                client,
                brain: brain.clone(),
            };
            let answer = session::answer(&text, brain, &thinker, &BusTools { client });
            Box::new(move |service: &mut Self, _: &Bus<'_, Self>| service.reply(id, &answer))
        });
        if started {
            self.asking.insert(id, request);
        } else {
            let _ = request.fail(ErrorCode::ServiceBusy);
        }
    }

    fn reply(&mut self, id: u64, answer: &Answer) {
        if let Some(request) = self.asking.remove(&id) {
            let _ = request.reply(answer);
        }
    }
}

impl Service for AgentService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![LlmState::TOPIC, MeshState::TOPIC]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        self.publish(bus);
        self.look(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
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
        if let Ok(state) = event.decode::<LlmState>() {
            self.local = state;
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
