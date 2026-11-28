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
    /// The last notice, with its button.
    pub notice: Option<Notice>,
    /// What was sent last (app, device), for "try again".
    pub last: Option<(String, Option<String>)>,
}

/// What the button of a notice does (board MeshStates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Take the waiting offer back.
    Cancel(u64),
    /// Send again.
    Retry,
    /// Open the file that arrived (its address).
    Open(String),
}

/// A short notice about an offer: what happened and, where it helps, what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Catalog key.
    pub key: &'static str,
    /// Until when it shows (ms).
    pub until: u64,
    /// Its button.
    pub act: Option<Act>,
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
/// How long "waiting for the other device" shows (the question there ends after a minute).
pub const WAIT_MS: u64 = 65_000;

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
        let (key, until, act) = match outcome {
            SendOutcome::Offered { offer, target } => {
                self.last = Some((app.to_owned(), Some(target.device)));
                (
                    "mobile.mesh.waiting",
                    now_ms.saturating_add(WAIT_MS),
                    Some(Act::Cancel(offer)),
                )
            }
            SendOutcome::NoTarget => ("mobile.mesh.no_target", now_ms + NOTICE_MS, None),
            SendOutcome::NoActivity => ("mobile.mesh.no_activity", now_ms + NOTICE_MS, None),
            SendOutcome::Choose(targets) => {
                self.choice = Some(Choice {
                    app: app.to_owned(),
                    targets,
                    chosen: 0,
                });
                return;
            }
        };
        self.notice = Some(Notice { key, until, act });
    }

    /// What became of the offers this device sent (`mesh.handoff_progress`): the end of an
    /// own offer replaces its "waiting" notice; one nobody answered or that was lost can be
    /// sent again.
    pub fn progress(&mut self, p: &HandoffProgress, now_ms: u64) {
        if !p.outgoing {
            return;
        }
        let (key, retry) = match p.state {
            OfferState::Opened => ("mobile.mesh.opened", false),
            OfferState::Declined => ("mobile.mesh.declined", false),
            OfferState::Expired => ("mobile.mesh.expired", true),
            OfferState::Failed => ("mobile.mesh.failed", true),
            OfferState::Asked | OfferState::Accepted => return,
        };
        let act = (retry && self.last.is_some()).then_some(Act::Retry);
        self.notice = Some(Notice {
            key,
            until: now_ms.saturating_add(NOTICE_MS),
            act,
        });
    }

    /// A file from another device arrived (`mesh.received`): tell, and offer to open it.
    pub fn received(&mut self, uri: String, now_ms: u64) {
        self.notice = Some(Notice {
            key: "mobile.mesh.received",
            until: now_ms.saturating_add(NOTICE_MS),
            act: Some(Act::Open(uri)),
        });
    }

    /// Takes the button of the notice away (it was pressed) and closes the notice.
    pub fn take_act(&mut self) -> Option<Act> {
        self.notice.take().and_then(|n| n.act)
    }

    /// The notice shown at `now_ms`, if any.
    pub fn notice_at(&self, now_ms: u64) -> Option<&'static str> {
        self.notice_card(now_ms).map(|n| n.key)
    }

    /// The whole notice shown at `now_ms`, if any.
    pub fn notice_card(&self, now_ms: u64) -> Option<&Notice> {
        self.notice.as_ref().filter(|n| now_ms < n.until)
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
