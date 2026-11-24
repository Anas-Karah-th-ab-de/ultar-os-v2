//! The mesh calls of the shell: the switch of the quick settings and "continue on another
//! device".

use super::bus::BusPorts;
use super::services::Answer;
use super::PortError;
use crate::model::services::Call;
use std::time::Duration;
use ultar_bus::messages::mesh::{Activity, HandoffSend, SendOutcome, SetActivity, SetEnabled};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

impl BusPorts {
    fn send(&self, target: &Option<String>) -> Result<SendOutcome, PortError> {
        let send = HandoffSend {
            target: target.clone(),
        };
        self.mesh_ask(&send)
    }

    fn mesh_ask<C: ultar_bus::Call>(&self, request: &C) -> Result<C::Reply, PortError> {
        let reply = self.client().call(request, CALL_TIMEOUT);
        reply.map_err(|_| PortError::Unavailable)
    }

    /// `mesh.set_enabled` and `mesh.handoff_send`. An app that reported nothing is offered
    /// as "this app, opened there": the shell says which app and title are in front.
    pub(super) fn mesh(&self, call: &Call) -> Result<Answer, PortError> {
        match call {
            Call::SetMesh(enabled) => {
                let enabled = *enabled;
                let done = self.mesh_ask(&SetEnabled { enabled })?.ok;
                Ok(if done { Answer::Done } else { Answer::NotDone })
            }
            Call::MeshSend(target, front) => {
                let first = self.send(target)?;
                let (SendOutcome::NoActivity, Some((app, title))) = (&first, front) else {
                    return Ok(Answer::Sent(first));
                };
                let activity = Activity {
                    app: app.clone(),
                    title: title.clone(),
                    uri: None,
                    file: None,
                };
                let set = SetActivity {
                    activity: Some(activity),
                };
                if !self.mesh_ask(&set)?.ok {
                    return Ok(Answer::Sent(first));
                }
                let second = self.send(target);
                // what the shell said is taken back: the next app in front speaks for itself
                let _ = self.mesh_ask(&SetActivity { activity: None });
                second.map(Answer::Sent)
            }
            _ => Ok(Answer::NotDone),
        }
    }
}
