//! The bus side of acting on another device: `mesh.act` sends a request to a paired
//! device's helper, and a paired device's request is given to this device's helper
//! (`agent.ask`).

use crate::node::Effect;
use crate::service::MeshService;
use std::time::{Duration, Instant};
use ultar_bus::messages::agent::{Ask, Step};
use ultar_bus::messages::mesh_remote::{Act, Acted};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::Bus;

/// How long the helper here may take for a paired device's request.
pub const HELPER_TIME: Duration = Duration::from_secs(140);
/// How long the asking side waits for the other device.
pub const ACT_TIME: Duration = Duration::from_secs(150);

impl MeshService {
    /// A paired device asks this device's helper.
    fn serve_remote(&mut self, peer: String, id: u64, text: String, bus: &Bus<'_, Self>) {
        let to = peer.clone();
        let started = bus.later(move |client| {
            let answer = client.call(&Ask { text }, HELPER_TIME).ok();
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                let (ok, steps) = answer.map_or((false, Vec::new()), |a| (true, a.steps));
                let effects = service.runtime.node.acted(&to, id, ok, steps);
                service.act(effects, bus);
            })
        });
        if !started {
            let effects = self.runtime.node.acted(&peer, id, false, Vec::new());
            self.act(effects, bus);
        }
    }

    fn remote_answered(&mut self, id: u64, by: String, ok: bool, steps: Vec<Step>) {
        if let Some((request, _)) = self.acting.remove(&id) {
            let _ = request.reply(&Acted { ok, steps, by });
        }
    }

    /// Requests no device answered in time get "no".
    pub(crate) fn expire_acting(&mut self, now: Instant) {
        let late: Vec<u64> = self
            .acting
            .iter()
            .filter(|(_, (_, until))| now >= *until)
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            self.remote_answered(id, String::new(), false, Vec::new());
        }
    }

    /// `mesh.act`; gives the request back when it is another method.
    pub(crate) fn remote(
        &mut self,
        request: IncomingRequest,
        bus: &Bus<'_, Self>,
    ) -> Option<IncomingRequest> {
        if request.method() != Act::METHOD {
            return Some(request);
        }
        let Ok(act) = request.decode::<Act>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return None;
        };
        match self.runtime.node.act(&act.device, &act.text) {
            Some((id, effects)) => {
                self.acting.insert(id, (request, Instant::now() + ACT_TIME));
                self.act(effects, bus);
            }
            None => {
                let _ = request.reply(&Acted::default());
            }
        }
        None
    }

    /// What is left of the effects for the host after the model's.
    pub(crate) fn remote_effect(&mut self, effect: Effect, bus: &Bus<'_, Self>) {
        match effect {
            Effect::Act { peer, id, text } => self.serve_remote(peer, id, text, bus),
            Effect::Acted {
                id,
                peer,
                ok,
                steps,
            } => self.remote_answered(id, peer, ok, steps),
            other => self.view_effect(other, bus),
        }
    }
}
