//! The mesh as the shell sees it (docs/ui/mesh.md §2.5, §2.6): whether something can be
//! continued on another device, what is in front, the choice of devices (board MeshSend)
//! and the toasts about an offer (board MeshStates: waiting with Cancel, opened, declined,
//! nobody answered and connection lost with Retry, a file arrived with Open).

use super::services::Services;
use super::{Desktop, Open};
use ultar_bus::messages::mesh::{
    HandoffProgress, MeshState, OfferState, SendOutcome, Stage, Target,
};
use ultar_ui::widgets::{Tone, MAX_TOASTS};

/// How long a toast with a button stays (the toolkit's `motion.toast_action`, ms).
pub const TOAST_LIFE_MS: u64 = 10_000;
/// A toast that closes this much before its time is over closed by itself.
const TIMEOUT_SLACK_MS: u64 = 500;

/// What a toast says (its text is the catalog key `desk.mesh.toast.<kind>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The offer is on its way and the other device asks its person.
    Waiting,
    /// The activity is open on the other device.
    Opened,
    /// The other person declined.
    Declined,
    /// Nobody answered within a minute.
    Expired,
    /// The connection was lost, nothing was stored there.
    Failed,
    /// A file arrived from another device.
    Received,
    /// No device can take it now.
    NoTarget,
    /// Nothing in front can be continued.
    NoActivity,
}

impl Kind {
    /// The meaning (icon and colour) of the toast.
    pub fn tone(self) -> Tone {
        match self {
            Kind::Waiting | Kind::NoActivity => Tone::Info,
            Kind::Opened | Kind::Received => Tone::Success,
            Kind::Declined | Kind::Expired | Kind::NoTarget => Tone::Warning,
            Kind::Failed => Tone::Danger,
        }
    }

    /// The catalog key of its one line.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Waiting => "desk.mesh.toast.waiting",
            Kind::Opened => "desk.mesh.toast.opened",
            Kind::Declined => "desk.mesh.toast.declined",
            Kind::Expired => "desk.mesh.toast.expired",
            Kind::Failed => "desk.mesh.toast.failed",
            Kind::Received => "desk.mesh.toast.received",
            Kind::NoTarget => "desk.mesh.toast.no_target",
            Kind::NoActivity => "desk.mesh.toast.no_activity",
        }
    }
}

/// The button of a toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Take the waiting offer back (`mesh.offer_cancel`).
    Cancel(u64),
    /// Send the offer to this device again (`mesh.handoff_send`).
    Retry(String),
    /// Open what arrived (`launcher.start` with this URI).
    Open(String),
}

impl Act {
    /// The catalog key of the button's label.
    pub fn key(&self) -> &'static str {
        match self {
            Act::Cancel(_) => "desk.mesh.cancel",
            Act::Retry(_) => "desk.mesh.retry",
            Act::Open(_) => "desk.mesh.open",
        }
    }
}

/// One toast of the mesh (board MeshStates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    /// Its id on the toasts' surface.
    pub id: u32,
    /// What it says.
    pub kind: Kind,
    /// The other device's name (data).
    pub name: String,
    /// What was offered or arrived (data).
    pub title: String,
    /// Its button.
    pub act: Option<Act>,
}

/// An offer this device sent: who got it and what it was.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sent {
    offer: u64,
    device: String,
    name: String,
    title: String,
}

/// What the shell knows of the mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// `mesh.state` (`None`: no mesh service on this device).
    pub state: Option<MeshState>,
    /// The devices the person chooses from (the mesh answered `Choose`).
    pub targets: Vec<Target>,
    /// The chosen device (index into `targets`).
    pub chosen: usize,
    /// The toasts shown, oldest first.
    pub toasts: Vec<Toast>,
    /// When the toasts' surface was built (ms): their clocks start then.
    pub built_at: u64,
    next_toast: u32,
    sent: Vec<Sent>,
}

impl Mesh {
    /// The id of the chosen device, if the choice is open and valid.
    pub fn confirm(&self) -> Option<String> {
        self.targets.get(self.chosen).map(|t| t.device.clone())
    }

    /// Chooses the device `index` of the choice; `false` when there is no such device.
    pub fn choose(&mut self, index: usize) -> bool {
        if index >= self.targets.len() {
            return false;
        }
        self.chosen = index;
        true
    }

    /// Shows a toast (the oldest goes beyond the toolkit's limit).
    pub fn toast(&mut self, kind: Kind, name: &str, title: &str, act: Option<Act>) {
        self.next_toast = self.next_toast.wrapping_add(1);
        let name: String = name.chars().take(64).collect();
        let title: String = title.chars().take(96).collect();
        self.toasts.push(Toast {
            id: self.next_toast,
            kind,
            name,
            title,
            act,
        });
        while self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    /// A toast's button was pressed: the toast goes and its button's meaning is returned.
    pub fn take_act(&mut self, id: u32) -> Option<Act> {
        let at = self.toasts.iter().position(|t| t.id == id)?;
        self.toasts.remove(at).act
    }

    /// The toolkit closed a toast (the person, or its time). Returns whether the toasts'
    /// surface must be built again. A waiting toast that only timed out stays: the offer
    /// still waits and the person must be able to take it back.
    pub fn closed(&mut self, id: u32, now: u64) -> bool {
        let Some(at) = self.toasts.iter().position(|t| t.id == id) else {
            return false;
        };
        let timed_out = now.saturating_sub(self.built_at) + TIMEOUT_SLACK_MS >= TOAST_LIFE_MS;
        if self.toasts[at].kind == Kind::Waiting && timed_out {
            return true;
        }
        self.toasts.remove(at);
        true
    }

    /// Forgets the waiting toast of an offer (its end arrived).
    fn end_waiting(&mut self, offer: u64) {
        self.toasts.retain(|t| t.act != Some(Act::Cancel(offer)));
    }

    /// A file arrived from another device (`mesh.received`): the toast with Open.
    pub fn received(&mut self, from: &str, title: &str, uri: Option<String>) {
        self.toast(Kind::Received, from, title, uri.map(Act::Open));
    }
}

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

/// What will happen on a device, as the line of the choice (board MeshSend: name, then
/// this line below it; `{ $app }` is the app that is offered).
pub fn stage_key(stage: Stage) -> &'static str {
    match stage {
        Stage::State => "desk.mesh.target.state",
        Stage::File => "desk.mesh.target.file",
        Stage::RemoteView => "desk.mesh.target.view",
    }
}

/// The name of a device as the person knows it.
fn name_of(svc: &Services, device: &str) -> String {
    svc.mesh
        .state
        .iter()
        .flat_map(|m| &m.peers)
        .find(|p| p.id == device)
        .map_or_else(|| device.to_owned(), |p| p.name.clone())
}

/// The mesh answered "continue on another device": a toast, or the choice of devices.
pub fn sent(d: &mut Desktop, outcome: SendOutcome) {
    let title = in_front(d).map(|(_, t)| t).unwrap_or_default();
    let mesh = &mut d.svc.mesh;
    match outcome {
        // Waiting: the sender can take the offer back (mesh.offer_cancel).
        SendOutcome::Offered { offer, target } => {
            mesh.sent.retain(|s| s.offer != offer);
            mesh.sent.push(Sent {
                offer,
                device: target.device.clone(),
                name: target.name.clone(),
                title: title.clone(),
            });
            let cancel = Some(Act::Cancel(offer));
            mesh.toast(Kind::Waiting, &target.name, &title, cancel);
        }
        SendOutcome::NoTarget => mesh.toast(Kind::NoTarget, "", "", None),
        SendOutcome::NoActivity => mesh.toast(Kind::NoActivity, "", "", None),
        SendOutcome::Choose(targets) => {
            mesh.targets = targets;
            mesh.chosen = 0;
            d.open = Open::MeshSend;
        }
    }
}

/// What became of the offers this device sent (`mesh.handoff_progress`).
pub fn report(d: &mut Desktop, progress: &[HandoffProgress]) {
    for p in progress.iter().filter(|p| p.outgoing) {
        let kind = match p.state {
            OfferState::Opened => Kind::Opened,
            OfferState::Declined => Kind::Declined,
            OfferState::Expired => Kind::Expired,
            OfferState::Failed => Kind::Failed,
            OfferState::Asked | OfferState::Accepted => continue,
        };
        let known = d.svc.mesh.sent.iter().find(|s| s.offer == p.offer).cloned();
        let name = known
            .as_ref()
            .map_or_else(|| name_of(&d.svc, &p.device), |s| s.name.clone());
        let title = known.map(|s| s.title).unwrap_or_default();
        let mesh = &mut d.svc.mesh;
        mesh.end_waiting(p.offer);
        mesh.sent.retain(|s| s.offer != p.offer);
        // Nobody answered or the connection was lost: one click sends the offer again.
        let retry = matches!(kind, Kind::Expired | Kind::Failed);
        let act = retry.then(|| Act::Retry(p.device.clone()));
        mesh.toast(kind, &name, &title, act);
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

    fn target(device: &str) -> Target {
        Target {
            device: device.into(),
            name: device.into(),
            kind: DeviceKind::Phone,
            stage: Stage::State,
        }
    }

    fn progress(state: OfferState) -> HandoffProgress {
        HandoffProgress {
            offer: 1,
            device: "p".into(),
            outgoing: true,
            state,
        }
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
    fn several_possible_devices_open_the_choice_and_one_answer_is_a_toast() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        sent(&mut d, SendOutcome::Choose(vec![target("a"), target("b")]));
        assert_eq!(d.open, Open::MeshSend);
        assert_eq!(d.svc.mesh.targets.len(), 2);
        assert_eq!(d.svc.mesh.chosen, 0);
        assert!(d.svc.mesh.choose(1) && !d.svc.mesh.choose(2));
        assert_eq!(d.svc.mesh.confirm().as_deref(), Some("b"));
        assert!(d.svc.mesh.toasts.is_empty());
        sent(&mut d, SendOutcome::NoActivity);
        assert_eq!(d.svc.mesh.toasts.len(), 1);
        assert_eq!(d.svc.mesh.toasts[0].kind, Kind::NoActivity);
        sent(&mut d, SendOutcome::NoTarget);
        assert_eq!(d.svc.mesh.toasts[1].kind, Kind::NoTarget);
    }

    #[test]
    fn only_the_end_of_an_own_offer_is_reported() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        let quiet = [
            progress(OfferState::Asked),
            HandoffProgress {
                outgoing: false,
                ..progress(OfferState::Opened)
            },
        ];
        report(&mut d, &quiet);
        assert!(d.svc.mesh.toasts.is_empty());
        report(&mut d, &[progress(OfferState::Declined)]);
        assert_eq!(d.svc.mesh.toasts.len(), 1);
        assert_eq!(d.svc.mesh.toasts[0].kind, Kind::Declined);
        assert_eq!(d.svc.mesh.toasts[0].act, None);
    }

    #[test]
    fn waiting_has_cancel_and_its_end_replaces_it_with_retry_where_it_helps() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        let offer = SendOutcome::Offered {
            offer: 1,
            target: target("p"),
        };
        sent(&mut d, offer);
        let waiting = d.svc.mesh.toasts[0].clone();
        assert_eq!(waiting.kind, Kind::Waiting);
        assert_eq!(waiting.act, Some(Act::Cancel(1)));
        report(&mut d, &[progress(OfferState::Expired)]);
        assert_eq!(d.svc.mesh.toasts.len(), 1, "the waiting toast is gone");
        let expired = &d.svc.mesh.toasts[0];
        assert_eq!((expired.kind, expired.act.clone()), (Kind::Expired, Some(Act::Retry("p".into()))));
        report(&mut d, &[progress(OfferState::Failed)]);
        assert_eq!(d.svc.mesh.toasts[1].act, Some(Act::Retry("p".into())));
        report(&mut d, &[progress(OfferState::Opened)]);
        assert_eq!(d.svc.mesh.toasts[2].act, None);
    }

    #[test]
    fn a_waiting_toast_that_only_timed_out_stays_but_a_closed_one_goes() {
        let mut m = Mesh::default();
        m.toast(Kind::Waiting, "KM One", "notes.txt", Some(Act::Cancel(7)));
        let id = m.toasts[0].id;
        m.built_at = 1_000;
        // The person closed it after two seconds.
        assert!(m.closed(id, 3_000) && m.toasts.is_empty());
        m.toast(Kind::Waiting, "KM One", "notes.txt", Some(Act::Cancel(7)));
        let id = m.toasts[0].id;
        // Its ten seconds ran out: the offer still waits, the toast comes back.
        assert!(m.closed(id, 1_000 + TOAST_LIFE_MS));
        assert_eq!(m.toasts.len(), 1);
        // Pressing Cancel takes it, whatever the clock says.
        assert_eq!(m.take_act(id), Some(Act::Cancel(7)));
        assert!(m.toasts.is_empty() && !m.closed(id, 0));
        // Other toasts always go when they close.
        m.toast(Kind::Declined, "KM One", "", None);
        let id = m.toasts[0].id;
        assert!(m.closed(id, 1_000 + TOAST_LIFE_MS) && m.toasts.is_empty());
    }

    #[test]
    fn no_more_toasts_than_the_toolkit_shows() {
        let mut m = Mesh::default();
        for _ in 0..5 {
            m.toast(Kind::Opened, "x", "y", None);
        }
        assert_eq!(m.toasts.len(), MAX_TOASTS);
        assert_eq!(m.toasts[0].id, 3, "the oldest went");
    }

    #[test]
    fn a_received_file_offers_open_when_its_place_is_known() {
        let mut m = Mesh::default();
        m.received("beelink", "report.pdf", Some("file:///data/inbox/report.pdf".into()));
        m.received("beelink", "b.pdf", None);
        assert_eq!(
            m.toasts[0].act,
            Some(Act::Open("file:///data/inbox/report.pdf".into()))
        );
        assert_eq!(m.toasts[1].act, None);
    }
}
