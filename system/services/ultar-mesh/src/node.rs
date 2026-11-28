//! One device of the mesh as a state machine: links to other devices, pairing, the
//! clipboard and (in [`crate::offers`]) handing work over. It does no input or output of
//! its own – it is told what happened and answers with [`Effect`]s – so two nodes can be
//! wired together in a test exactly as two devices are over the network.

use crate::device::{self, Device};
use crate::offers::Offers;
use crate::trust::{Joiner, Shower, Shown, Trust};
use crate::wire::{Frame, VERSION};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;
use ultar_bus::messages::agent::Step;
use ultar_bus::messages::mesh::{
    Activity, Capability, ClipboardRemote, DeviceKind, HandoffIncoming, HandoffProgress, MeshState,
    Peer, Right,
};

/// What this device is.
#[derive(Clone, Debug)]
pub struct Config {
    /// Its name (data).
    pub name: String,
    /// Its kind.
    pub kind: DeviceKind,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Ids of the installed apps.
    pub apps: Vec<String>,
    /// Where received files go.
    pub inbox: PathBuf,
    /// Folders whose files may be sent (the person's own folders). Empty: none.
    pub share: Vec<PathBuf>,
    /// It can show its windows on another device.
    pub remote_view: bool,
}

/// What the node wants done.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    /// Send a frame to a linked device.
    Send(String, Frame),
    /// Ask the person: another device offers something.
    Incoming(HandoffIncoming),
    /// An offer changed.
    Progress(HandoffProgress),
    /// Open what arrived (`app` empty: with the default app); report with
    /// [`Node::opened`].
    Open {
        /// The offer.
        offer: u64,
        /// App id.
        app: String,
        /// What to open.
        uri: Option<String>,
    },
    /// Text copied on another device.
    Clipboard(ClipboardRemote),
    /// A job to run for `peer`; report with [`Node::job_done`].
    Run {
        /// Who asked.
        peer: String,
        /// The job.
        id: String,
        /// The program.
        program: String,
        /// Its arguments.
        args: Vec<String>,
    },
    /// A job this device asked for made progress or ended.
    Job(String, Frame),
    /// Let the local model answer for `peer`; report with [`Node::thought`].
    Think {
        /// Who asked.
        peer: String,
        /// The request.
        id: u64,
        /// What the model is and may do.
        system: String,
        /// What it is asked.
        prompt: String,
        /// Longest answer in tokens.
        max_tokens: u32,
    },
    /// A paired device answered a request of [`Node::think`].
    Thought {
        /// The request.
        id: u64,
        /// The device.
        peer: String,
        /// Its model answered.
        ok: bool,
        /// The answer.
        text: String,
    },
    /// Let the local helper do this for `peer`; report with [`Node::acted`].
    Act {
        /// Who asked.
        peer: String,
        /// The request.
        id: u64,
        /// What to do.
        text: String,
    },
    /// A paired device's helper answered a request of [`Node::act`].
    Acted {
        /// The request.
        id: u64,
        /// The device.
        peer: String,
        /// It took the request.
        ok: bool,
        /// What it did.
        steps: Vec<Step>,
    },
    /// A paired device offers a call; the person answers with [`Node::call_answer`].
    CallIncoming(ultar_bus::messages::mesh_calls::CallIncoming),
    /// A call changed on this device.
    CallProgress(ultar_bus::messages::mesh_calls::CallProgress),
    /// One 20 ms of a call's voice from a paired device.
    CallAudio(ultar_bus::messages::mesh_calls::CallAudio),
    /// A notification of a paired device to show here.
    Note(ultar_bus::messages::mesh_calls::NoteRemote),
    /// The picture of the mesh changed ([`Node::state`]).
    Changed,
}

pub(crate) struct Link {
    pub(crate) key: Vec<u8>,
    pub(crate) device: Option<Device>,
    pub(crate) joiner: Option<Joiner>,
    pub(crate) shower: Option<Shower>,
}

/// One device of the mesh.
pub struct Node {
    pub(crate) id: String,
    pub(crate) key: Vec<u8>,
    pub(crate) config: Config,
    enabled: bool,
    pub(crate) trust: Trust,
    pub(crate) links: BTreeMap<String, Link>,
    pub(crate) shown: Option<Shown>,
    pub(crate) activity: Option<Activity>,
    pub(crate) reported: BTreeMap<String, Activity>,
    pub(crate) offers: Offers,
    pub(crate) clip_seq: u64,
    pub(crate) clip_remote: Option<String>,
    pub(crate) thinking: u64,
    pub(crate) serving: u32,
    pub(crate) calls: crate::calls::Calls,
}

impl Node {
    /// The device with the static public key `key`.
    pub fn new(config: Config, key: &[u8], trust: Trust) -> Self {
        Self {
            id: device::id_of(key),
            key: key.to_owned(),
            config,
            enabled: true,
            trust,
            links: BTreeMap::new(),
            shown: None,
            activity: None,
            reported: BTreeMap::new(),
            offers: Offers::default(),
            clip_seq: 0,
            clip_remote: None,
            thinking: 0,
            serving: 0,
            calls: crate::calls::Calls::default(),
        }
    }

    /// This device's id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Sets the ids of the installed apps (told to devices linked from now on).
    pub fn set_apps(&mut self, apps: Vec<String>) {
        self.config.apps = apps;
    }

    /// `true` while the mesh is switched on.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Switches the mesh on or off; off drops every link.
    pub fn set_enabled(&mut self, on: bool) -> Vec<Effect> {
        self.enabled = on;
        let mut effects = Vec::new();
        if !on {
            self.shown = None;
            let peers: Vec<String> = self.links.keys().cloned().collect();
            for peer in peers {
                effects.extend(self.link_down(&peer));
            }
        }
        effects.push(Effect::Changed);
        effects
    }

    pub(crate) fn paired(&self, peer: &str) -> bool {
        self.links
            .get(peer)
            .is_some_and(|l| self.trust.knows(peer, &l.key))
    }

    pub(crate) fn allows(&self, peer: &str, right: Right) -> bool {
        self.paired(peer) && self.trust.allows(peer, right)
    }

    pub(crate) fn hello(&self) -> Frame {
        Frame::Hello {
            version: VERSION,
            name: device::clean_name(&self.config.name),
            kind: device::kind_word(self.config.kind).to_owned(),
            caps: self
                .config
                .capabilities
                .iter()
                .map(|c| device::capability_word(*c).to_owned())
                .collect(),
            apps: self.config.apps.clone(),
        }
    }

    /// An encrypted channel to the device with `key` is open.
    pub fn link_up(&mut self, peer: &str, key: &[u8]) -> Vec<Effect> {
        if !self.enabled || peer == self.id || device::id_of(key) != peer {
            return Vec::new();
        }
        let mut effects = self.link_down(peer);
        self.links.insert(
            peer.to_owned(),
            Link {
                key: key.to_owned(),
                device: None,
                joiner: None,
                shower: None,
            },
        );
        effects.push(Effect::Send(peer.to_owned(), self.hello()));
        effects.push(Effect::Changed);
        effects
    }

    /// The channel to `peer` is gone.
    pub fn link_down(&mut self, peer: &str) -> Vec<Effect> {
        if self.links.remove(peer).is_none() {
            return Vec::new();
        }
        let mut effects = self.drop_offers(peer);
        effects.extend(self.calls.peer_down(peer));
        effects.push(Effect::Changed);
        effects
    }

    /// The mesh as this device sees it.
    pub fn state(&self) -> MeshState {
        let mut peers: Vec<Peer> = Vec::new();
        for (id, link) in &self.links {
            let paired = self.trust.knows(id, &link.key);
            let known = link.device.as_ref();
            peers.push(Peer {
                id: id.clone(),
                name: known.map_or_else(String::new, |d| d.name.clone()),
                kind: known.map_or(DeviceKind::Other, |d| d.kind),
                capabilities: known.map_or_else(Vec::new, |d| d.capabilities.clone()),
                paired,
                reachable: true,
                rights: self
                    .trust
                    .get(id)
                    .filter(|_| paired)
                    .map_or_else(Vec::new, |p| p.rights.clone()),
            });
        }
        for (id, paired) in self.trust.all() {
            if !self.links.contains_key(id) {
                peers.push(Peer {
                    id: id.clone(),
                    name: paired.name.clone(),
                    kind: device::kind_from(&paired.kind),
                    capabilities: Vec::new(),
                    paired: true,
                    reachable: false,
                    rights: paired.rights.clone(),
                });
            }
        }
        MeshState {
            enabled: self.enabled,
            id: self.id.clone(),
            name: device::clean_name(&self.config.name),
            peers,
        }
    }

    /// Forgets the paired device `peer`.
    pub fn unpair(&mut self, peer: &str) -> (bool, Vec<Effect>) {
        if !self.trust.remove(peer) {
            return (false, Vec::new());
        }
        let mut effects = self.drop_offers(peer);
        effects.push(Effect::Changed);
        (true, effects)
    }

    /// Sets what the paired device `peer` may do here.
    pub fn set_rights(&mut self, peer: &str, rights: &[Right]) -> (bool, Vec<Effect>) {
        if !self.trust.set_rights(peer, rights) {
            return (false, Vec::new());
        }
        (true, vec![Effect::Changed])
    }

    fn on_hello(&mut self, peer: &str, frame: Frame) -> Vec<Effect> {
        let Frame::Hello {
            name,
            kind,
            caps,
            apps,
            ..
        } = frame
        else {
            return Vec::new();
        };
        let name = device::clean_name(&name);
        self.trust.seen(peer, &name, &kind);
        if let Some(link) = self.links.get_mut(peer) {
            link.device = Some(Device {
                id: peer.to_owned(),
                name,
                kind: device::kind_from(&kind),
                capabilities: device::capabilities_from(&caps),
                apps: apps.into_iter().take(512).collect(),
            });
        }
        vec![Effect::Changed]
    }

    /// A frame arrived from the linked device `peer`.
    pub fn frame(&mut self, peer: &str, frame: Frame, now: Instant) -> Vec<Effect> {
        if !self.enabled || !self.links.contains_key(peer) {
            return Vec::new();
        }
        match frame {
            Frame::Hello { .. } => return self.on_hello(peer, frame),
            Frame::PairStart { .. }
            | Frame::PairReply { .. }
            | Frame::PairConfirm { .. }
            | Frame::PairRefused => return self.on_pairing(peer, frame, now),
            _ => {}
        }
        // Everything else is for paired devices only.
        if !self.paired(peer) {
            return Vec::new();
        }
        match frame {
            Frame::Clip { text, .. } => self.on_clip(peer, text),
            Frame::Job { id, program, args } => self.on_job(peer, id, program, args),
            Frame::JobAlive { .. } | Frame::JobDone { .. } | Frame::JobRefused { .. } => {
                vec![Effect::Job(peer.to_owned(), frame)]
            }
            Frame::Think { .. } | Frame::Thought { .. } => self.on_model_frame(peer, frame),
            Frame::Act { .. } | Frame::Acted { .. } => self.on_remote_frame(peer, frame),
            Frame::Call { .. }
            | Frame::CallAnswer { .. }
            | Frame::CallEnd { .. }
            | Frame::CallAudio { .. }
            | Frame::Note { .. } => self.on_call_frame(peer, frame),
            other => self.on_offer_frame(peer, other, now),
        }
    }

    fn on_job(
        &mut self,
        peer: &str,
        id: String,
        program: String,
        args: Vec<String>,
    ) -> Vec<Effect> {
        let can = self.config.capabilities.contains(&Capability::Compute);
        if !can || !self.allows(peer, Right::Compute) {
            return vec![Effect::Send(peer.to_owned(), Frame::JobRefused { id })];
        }
        vec![Effect::Run {
            peer: peer.to_owned(),
            id,
            program,
            args,
        }]
    }

    /// Regular work: offers nobody answered, a code nobody used.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        if self.shown.as_ref().is_some_and(|s| !s.valid(now)) {
            self.shown = None;
        }
        self.expire_offers(now)
    }
}
