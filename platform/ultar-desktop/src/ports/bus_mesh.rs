//! The mesh and helper calls of the shell: the switch of the quick settings, "continue on
//! another device", and a request to the helper.

use super::bus::BusPorts;
use super::services::Answer;
use super::PortError;
use crate::model::services::Call;
use std::time::Duration;
use ultar_bus::messages::mesh::{
    Activity, HandoffSend, OfferCancel, SendOutcome, SetActivity, SetEnabled,
};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The helper may think (also on another device) and act in several steps.
const HELPER_TIMEOUT: Duration = Duration::from_secs(170);

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

    /// `mesh.set_enabled` and `mesh.handoff_send`; before sending, the shell says which app
    /// and title are in front.
    pub(super) fn mesh(&self, call: &Call) -> Result<Answer, PortError> {
        match call {
            Call::SetMesh(enabled) => {
                let enabled = *enabled;
                let done = self.mesh_ask(&SetEnabled { enabled })?.ok;
                Ok(if done { Answer::Done } else { Answer::NotDone })
            }
            Call::MeshSend(target, front) => {
                // the mesh offers what that app reported, or the app as it is
                if let Some((app, title)) = front {
                    let activity = Activity {
                        app: app.clone(),
                        title: title.clone(),
                        uri: None,
                        file: None,
                    };
                    let set = SetActivity {
                        activity: Some(activity),
                    };
                    self.mesh_ask(&set)?;
                }
                self.send(target).map(Answer::Sent)
            }
            Call::OfferCancel(offer) => {
                let cancel = OfferCancel { offer: *offer };
                let done = self.mesh_ask(&cancel)?.ok;
                Ok(if done { Answer::Done } else { Answer::NotDone })
            }
            Call::AskHelper(text) => {
                let ask = ultar_bus::messages::agent::Ask { text: text.clone() };
                let reply = self.client().call(&ask, HELPER_TIMEOUT);
                reply
                    .map(Answer::Helper)
                    .map_err(|_| PortError::Unavailable)
            }
            _ => Ok(Answer::NotDone),
        }
    }
}
