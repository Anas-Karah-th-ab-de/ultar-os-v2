//! The bus side of lending the language model: this device's model answers a paired
//! device (`llm.complete`), and `mesh.think` lets a paired device's model answer here.

use crate::node::Effect;
use crate::service::MeshService;
use std::time::{Duration, Instant};
use ultar_bus::messages::llm::{Complete, LlmState};
use ultar_bus::messages::mesh_model::{ModelList, Models, Think, Thought};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::Bus;

/// How long a model may take for one answer.
pub const THINK_TIME: Duration = Duration::from_secs(120);

impl MeshService {
    /// The model of this device changed (`llm.state`).
    pub(crate) fn model_changed(&mut self, state: &LlmState, bus: &Bus<'_, Self>) {
        let class = state.ready.then_some(state.class);
        let effects = self.runtime.node.set_model(class);
        self.act(effects, bus);
    }

    /// A paired device asks this device's model.
    pub(crate) fn lend(&mut self, peer: String, id: u64, ask: Complete, bus: &Bus<'_, Self>) {
        let to = peer.clone();
        let started = bus.later(move |client| {
            let answer = client.call(&ask, THINK_TIME).ok().filter(|a| a.ok);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                let (ok, text) = answer.map_or((false, String::new()), |a| (true, a.text));
                let effects = service.runtime.node.thought(&to, id, ok, text);
                service.act(effects, bus);
            })
        });
        if !started {
            let effects = self.runtime.node.thought(&peer, id, false, String::new());
            self.act(effects, bus);
        }
    }

    /// A paired device's model answered request `id`.
    pub(crate) fn answered(&mut self, id: u64, peer: String, ok: bool, text: String) {
        if let Some((request, _)) = self.waiting.remove(&id) {
            let _ = request.reply(&Thought { ok, text, by: peer });
        }
    }

    /// Requests no device answered in time get "no".
    pub(crate) fn expire_thinking(&mut self, now: Instant) {
        let late: Vec<u64> = self
            .waiting
            .iter()
            .filter(|(_, (_, until))| now >= *until)
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            self.answered(id, String::new(), false, String::new());
        }
    }

    /// `mesh.think` and `mesh.models`; gives the request back when it is neither.
    pub(crate) fn lending(
        &mut self,
        request: IncomingRequest,
        bus: &Bus<'_, Self>,
    ) -> Option<IncomingRequest> {
        if request.method() == Models::METHOD {
            let lenders = self.runtime.node.lenders();
            let _ = request.reply(&ModelList { lenders });
            return None;
        }
        if request.method() != Think::METHOD {
            return Some(request);
        }
        let Ok(think) = request.decode::<Think>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return None;
        };
        let node = &mut self.runtime.node;
        let sent = node.think(
            think.device.as_deref(),
            &think.system,
            &think.prompt,
            think.max_tokens,
        );
        match sent {
            Some((id, _, effects)) => {
                self.waiting
                    .insert(id, (request, Instant::now() + THINK_TIME));
                self.act(effects, bus);
            }
            None => {
                let _ = request.reply(&Thought::default());
            }
        }
        None
    }

    /// What is left of the model effects for the host.
    pub(crate) fn model_effect(&mut self, effect: Effect, bus: &Bus<'_, Self>) {
        match effect {
            Effect::Think {
                peer,
                id,
                system,
                prompt,
                max_tokens,
            } => {
                let ask = Complete {
                    system,
                    prompt,
                    max_tokens,
                };
                self.lend(peer, id, ask, bus);
            }
            Effect::Thought { id, peer, ok, text } => self.answered(id, peer, ok, text),
            _ => {}
        }
    }
}
