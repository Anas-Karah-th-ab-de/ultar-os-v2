//! The mesh as the shell sees it (docs/ui/mesh.md §2.5, §2.6): whether something can be
//! continued on another device, what is in front, and the notices about an offer.

use super::services::Services;
use super::{Desktop, Open};
use ultar_bus::messages::mesh::{
    HandoffProgress, MeshState, OfferState, SendOutcome, Stage, Target,
};

/// What the shell knows of the mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// `mesh.state` (`None`: no mesh service on this device).
    pub state: Option<MeshState>,
    /// The devices the person chooses from (the mesh asked: more than one can take it).
    pub targets: Vec<Target>,
}

/// The shell's own notices.
const SHELL: &str = "org.ultar.shell";

/// The mesh is on and a paired device is connected.
pub fn can_send(svc: &Services) -> bool {
    svc.mesh
        .state
        .as_ref()
        .is_some_and(|m| m.enabled && m.peers.iter().any(|p| p.paired && p.reachable))
}

/// The app and title in front: what is offered when the app itself reported nothing.
pub fn in_front(d: &Desktop) -> Option<(String, String)> {
    let apps = d.bar_apps();
    let app = apps.iter().find(|t| t.active())?;
    let window = app.windows.iter().find(|w| w.focused)?;
    Some((app.app.id.clone(), window.title.clone()))
}

/// What will happen on a device, as the line of the choice.
pub fn stage_key(stage: Stage) -> &'static str {
    match stage {
        Stage::State => "desk.mesh.target.state",
        Stage::File => "desk.mesh.target.file",
        Stage::RemoteView => "desk.mesh.target.view",
    }
}

/// The mesh answered "continue on another device": a notice, or the choice of devices.
pub fn sent(d: &mut Desktop, outcome: SendOutcome, now: u64) {
    let notice = match outcome {
        SendOutcome::Offered { .. } => "desk.mesh.waiting",
        SendOutcome::NoTarget => "desk.mesh.no_target",
        SendOutcome::NoActivity => "desk.mesh.no_activity",
        SendOutcome::Choose(targets) => {
            d.svc.mesh.targets = targets;
            d.open = Open::AppMenu;
            return;
        }
    };
    d.notes.post_notice(SHELL, notice, now);
}

/// What became of the offers this device sent (`mesh.handoff_progress`).
pub fn report(d: &mut Desktop, progress: &[HandoffProgress], now: u64) {
    for p in progress.iter().filter(|p| p.outgoing) {
        let notice = match p.state {
            OfferState::Opened => "desk.mesh.opened",
            OfferState::Declined => "desk.mesh.declined",
            OfferState::Expired => "desk.mesh.expired",
            OfferState::Failed => "desk.mesh.failed",
            OfferState::Asked | OfferState::Accepted => continue,
        };
        d.notes.post_notice(SHELL, notice, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::mesh::{DeviceKind, Peer};

    fn peer(paired: bool, reachable: bool) -> Peer {
        Peer {
            id: "p".into(),
            name: "KM One".into(),
            kind: DeviceKind::Phone,
            capabilities: Vec::new(),
            paired,
            reachable,
            rights: Vec::new(),
        }
    }

    fn mesh(enabled: bool, peers: Vec<Peer>) -> Services {
        let state = MeshState {
            enabled,
            id: "me".into(),
            name: "pc".into(),
            peers,
        };
        let mut svc = Services::default();
        svc.mesh.state = Some(state);
        svc
    }

    #[test]
    fn sending_needs_the_mesh_on_and_a_connected_paired_device() {
        assert!(!can_send(&Services::default()));
        assert!(!can_send(&mesh(false, vec![peer(true, true)])));
        assert!(!can_send(&mesh(true, vec![peer(false, true)])));
        assert!(!can_send(&mesh(true, vec![peer(true, false)])));
        assert!(can_send(&mesh(true, vec![peer(true, true)])));
    }

    #[test]
    fn several_possible_devices_open_the_choice_and_one_answer_is_a_notice() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        let target = Target {
            device: "p".into(),
            name: "KM One".into(),
            kind: DeviceKind::Phone,
            stage: Stage::State,
        };
        sent(&mut d, SendOutcome::Choose(vec![target.clone(), target]), 5);
        assert_eq!(d.open, Open::AppMenu);
        assert_eq!(d.svc.mesh.targets.len(), 2);
        let before = d.notes.banners(5).len();
        sent(&mut d, SendOutcome::NoActivity, 5);
        assert_eq!(d.notes.banners(5).len(), before + 1);
    }

    #[test]
    fn only_the_end_of_an_own_offer_is_reported() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        let p = |outgoing, state| HandoffProgress {
            offer: 1,
            device: "p".into(),
            outgoing,
            state,
        };
        let quiet = [p(true, OfferState::Asked), p(false, OfferState::Opened)];
        report(&mut d, &quiet, 0);
        assert!(d.notes.banners(0).is_empty());
        report(&mut d, &[p(true, OfferState::Declined)], 0);
        assert_eq!(d.notes.banners(0).len(), 1);
    }
}
