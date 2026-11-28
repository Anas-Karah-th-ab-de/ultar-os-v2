//! The calls of the call banners (mesh item C2): answer or decline a call another device
//! offers, hang it up, mute its microphone.

use super::bus::BusPorts;
use super::services::Answer;
use super::PortError;
use crate::model::services::Call;
use std::time::Duration;
use ultar_bus::messages::mesh_calls::{CallAnswer, CallEnd, CallMeshMute};

/// The local services answer at once; the banner must not hang while one is missing.
const CALL_TIMEOUT: Duration = Duration::from_secs(2);

impl BusPorts {
    fn ask_call<C: ultar_bus::Call>(&self, request: &C) -> Result<C::Reply, PortError> {
        let reply = self.client().call(request, CALL_TIMEOUT);
        reply.map_err(|_| PortError::Unavailable)
    }

    /// `mesh.call_answer`, `mesh.call_end` and `call.mesh_mute`.
    pub(super) fn calls(&self, call: &Call) -> Result<Answer, PortError> {
        let ok = match call {
            Call::CallAnswer(id, accept) => {
                let answer = CallAnswer {
                    id: *id,
                    accept: *accept,
                };
                self.ask_call(&answer)?.ok
            }
            Call::CallEnd(id) => self.ask_call(&CallEnd { id: *id })?.ok,
            Call::CallMute(on) => self.ask_call(&CallMeshMute { on: *on })?.ok,
            _ => false,
        };
        Ok(if ok { Answer::Done } else { Answer::NotDone })
    }
}
