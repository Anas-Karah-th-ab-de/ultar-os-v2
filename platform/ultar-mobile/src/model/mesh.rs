//! The device mesh as the phone shell sees it (docs/ui/mesh.md §2.5, §2.6, board
//! CompactMeshSend): whether something can be continued on another device, the choice when
//! more than one device can take it, and the short notices about an offer. The question on
//! the receiving device is the system dialog's (`ultar-mobile-dialog`).

use ultar_bus::messages::mesh::{
    HandoffProgress, MeshState, OfferState, SendOutcome, Stage, Target,
};

/// What the shell knows of the mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// `mesh.state` (`None`: no mesh service on this device – no tile, no entry).
    pub state: Option<MeshState>,
    /// The choice: what is offered (app id) and the devices that can take it.
    pub choice: Option<Choice>,
    /// The last notice (catalog key, without `.body`) and until when it shows (ms).
    pub notice: Option<(&'static str, u64)>,
}

/// The choice on the sending device (two or more devices can take the activity).
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The app whose activity is offered.
    pub app: String,
    /// The devices, as the mesh listed them.
    pub targets: Vec<Target>,
    /// The chosen one (index into `targets`).
    pub chosen: usize,
}

/// How long a notice shows.
pub const NOTICE_MS: u64 = 6_000;

impl Mesh {
    /// The mesh service is there (tile and entries shown).
    pub fn present(&self) -> bool {
        self.state.is_some()
    }

    /// The mesh is on.
    pub fn enabled(&self) -> bool {
        self.state.as_ref().is_some_and(|m| m.enabled)
    }

    /// Paired devices that are connected now.
    pub fn connected(&self) -> usize {
        self.state.as_ref().map_or(0, |m| {
            m.peers.iter().filter(|p| p.paired && p.reachable).count()
        })
    }

    /// Something can be sent: the mesh is on and a paired device is connected.
    pub fn can_send(&self) -> bool {
        self.enabled() && self.connected() > 0
    }

    /// The mesh answered a send: a notice, or the choice of devices.
    pub fn sent(&mut self, app: &str, outcome: SendOutcome, now_ms: u64) {
        self.choice = None;
        let key = match outcome {
            SendOutcome::Offered { .. } => "mobile.mesh.waiting",
            SendOutcome::NoTarget => "mobile.mesh.no_target",
            SendOutcome::NoActivity => "mobile.mesh.no_activity",
            SendOutcome::Choose(targets) => {
                self.choice = Some(Choice {
                    app: app.to_owned(),
                    targets,
                    chosen: 0,
                });
                return;
            }
        };
        self.notice = Some((key, now_ms.saturating_add(NOTICE_MS)));
    }

    /// What became of the offers this device sent (`mesh.handoff_progress`).
    pub fn progress(&mut self, p: &HandoffProgress, now_ms: u64) {
        if !p.outgoing {
            return;
        }
        let key = match p.state {
            OfferState::Opened => "mobile.mesh.opened",
            OfferState::Declined => "mobile.mesh.declined",
            OfferState::Expired => "mobile.mesh.expired",
            OfferState::Failed => "mobile.mesh.failed",
            OfferState::Asked | OfferState::Accepted => return,
        };
        self.notice = Some((key, now_ms.saturating_add(NOTICE_MS)));
    }

    /// The notice shown at `now_ms`, if any.
    pub fn notice_at(&self, now_ms: u64) -> Option<&'static str> {
        self.notice
            .filter(|(_, until)| now_ms < *until)
            .map(|(k, _)| k)
    }
}

/// What will happen on a device, as the line of the choice.
pub fn stage_key(stage: Stage) -> &'static str {
    match stage {
        Stage::State => "mobile.mesh.target.state",
        Stage::File => "mobile.mesh.target.file",
        Stage::RemoteView => "mobile.mesh.target.view",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::mesh::{DeviceKind, Peer};

    fn peer(paired: bool, reachable: bool) -> Peer {
        Peer {
            id: "p".into(),
            name: "ultar-pc".into(),
            kind: DeviceKind::Desktop,
            capabilities: Vec::new(),
            paired,
            reachable,
            rights: Vec::new(),
        }
    }

    fn mesh(enabled: bool, peers: Vec<Peer>) -> Mesh {
        Mesh {
            state: Some(MeshState {
                enabled,
                id: "me".into(),
                name: "phone".into(),
                peers,
            }),
            ..Mesh::default()
        }
    }

    pub fn target(name: &str) -> Target {
        Target {
            device: name.into(),
            name: name.into(),
            kind: DeviceKind::Desktop,
            stage: Stage::State,
        }
    }

    #[test]
    fn sending_needs_the_mesh_on_and_a_connected_paired_device() {
        assert!(!Mesh::default().can_send() && !Mesh::default().present());
        assert!(!mesh(false, vec![peer(true, true)]).can_send());
        assert!(!mesh(true, vec![peer(false, true)]).can_send());
        assert!(!mesh(true, vec![peer(true, false)]).can_send());
        assert!(mesh(true, vec![peer(true, true)]).can_send());
    }

    #[test]
    fn several_devices_open_the_choice_one_answer_is_a_notice() {
        let mut m = mesh(true, vec![peer(true, true)]);
        m.sent(
            "org.ultar.editor",
            SendOutcome::Choose(vec![target("a"), target("b")]),
            0,
        );
        assert_eq!(m.choice.as_ref().map(|c| c.targets.len()), Some(2));
        m.sent("org.ultar.editor", SendOutcome::NoTarget, 10);
        assert!(m.choice.is_none());
        assert_eq!(m.notice_at(10), Some("mobile.mesh.no_target"));
        assert_eq!(m.notice_at(10 + NOTICE_MS), None);
    }

    #[test]
    fn only_the_end_of_an_own_offer_is_reported() {
        let mut m = Mesh::default();
        let p = |outgoing, state| HandoffProgress {
            offer: 1,
            device: "p".into(),
            outgoing,
            state,
        };
        m.progress(&p(true, OfferState::Asked), 0);
        m.progress(&p(false, OfferState::Opened), 0);
        assert_eq!(m.notice_at(0), None);
        m.progress(&p(true, OfferState::Declined), 0);
        assert_eq!(m.notice_at(0), Some("mobile.mesh.declined"));
    }
}
