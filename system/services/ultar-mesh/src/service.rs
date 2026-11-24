//! The bus side of the mesh: the calls of the shell, the settings and the apps, and what
//! the mesh asks of the system (asking the person, opening an app, the clipboard).

use crate::net::{Announcing, Net, NetEvent};
use crate::node::{Config, Effect, Node};
use crate::runtime::Runtime;
use crate::secure::Keys;
use crate::trust::Trust;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{List, Role, Start};
use ultar_bus::messages::llm::LlmState;
use ultar_bus::messages::mesh::{
    ClipboardSet, Done, HandoffAnswer, HandoffIncoming, HandoffSend, HandoffTargets, MeshState,
    PairBegin, PairJoin, SendFile, SetActivity, SetEnabled, SetRights, TargetList, Unpair, SERVICE,
};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_bus::messages::sysdialog;
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Bus, Service};

/// How often offers and codes are checked for their time.
pub const TICK: Duration = Duration::from_millis(500);
const CALL_TIME: Duration = Duration::from_secs(5);

/// Where the mesh listens and whom it looks for.
#[derive(Clone, Debug)]
pub struct Listen {
    /// Address of the channel port.
    pub bind: SocketAddr,
    /// The announcements (`None`: none).
    pub announce: Option<Announcing>,
    /// Folder with the programs computing jobs may run (`None`: no job runs here).
    pub jobs: Option<PathBuf>,
    /// Devices at known addresses (networks without broadcast).
    pub peers: Vec<SocketAddr>,
    /// The relay for devices in other networks (`None`: local network only).
    pub relay: Option<crate::relay::Login>,
    /// The app that opens a received file with its default app.
    pub opener: String,
}

/// The mesh service.
pub struct MeshService {
    pub(crate) runtime: Runtime,
    pub(crate) waiting: BTreeMap<u64, (IncomingRequest, Instant)>,
    pub(crate) acting: BTreeMap<u64, (IncomingRequest, Instant)>,
    inbox: Option<mpsc::Receiver<NetEvent>>,
    events: mpsc::Sender<NetEvent>,
    keys: Keys,
    listen: Listen,
    net: Option<Net>,
    last: Option<MeshState>,
}

/// The device's key pair in `file` (mode 0600), created on first start.
pub fn keys_in(file: &Path) -> Option<Keys> {
    if let Ok((private, public)) = ultar_svckit::store::load::<(Vec<u8>, Vec<u8>)>(file) {
        return Some(Keys { private, public });
    }
    let keys = Keys::generate()?;
    ultar_svckit::store::save(file, &(&keys.private, &keys.public)).ok()?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).ok()?;
    Some(keys)
}

fn serve<C: Call>(request: IncomingRequest, work: impl FnOnce(C, &str) -> C::Reply) {
    let caller = request.caller().to_owned();
    match request.decode::<C>() {
        Ok(call) => {
            let _ = request.reply(&work(call, &caller));
        }
        Err(_) => {
            let _ = request.fail(ErrorCode::BadPayload);
        }
    }
}

impl MeshService {
    /// The mesh of the device described by `config`, with its state below `state`.
    pub fn new(config: Config, state: &Path, listen: Listen) -> Option<Self> {
        std::fs::create_dir_all(state).ok()?;
        let keys = keys_in(&state.join("key"))?;
        let trust = Trust::open(&state.join("peers"));
        let node = Node::new(config, &keys.public, trust);
        let (events, inbox) = mpsc::channel();
        let mut runtime = Runtime::new(node, events.clone());
        runtime.set_jobs(listen.jobs.clone());
        Some(Self {
            runtime,
            waiting: BTreeMap::new(),
            acting: BTreeMap::new(),
            inbox: Some(inbox),
            events,
            keys,
            listen,
            net: None,
            last: None,
        })
    }

    fn open_net(&mut self, bus: &Bus<'_, Self>) {
        if self.net.is_some() {
            return;
        }
        let started = Net::start(
            self.keys.clone(),
            self.listen.bind,
            self.listen.announce,
            self.events.clone(),
        );
        match started {
            Ok(mut net) => {
                if let Some(login) = self.listen.relay.clone() {
                    net.use_relay(login);
                }
                for peer in &self.listen.peers {
                    net.connect(*peer);
                }
                self.net = Some(net);
            }
            Err(_) => bus.log(Record::new(Level::Warn, "mesh.listen_failed")),
        }
    }

    fn set_enabled(&mut self, on: bool, bus: &Bus<'_, Self>) -> bool {
        if on {
            let effects = self.runtime.set_enabled(true);
            self.host(effects, bus);
            self.open_net(bus);
            return self.net.is_some();
        }
        if let Some(net) = self.net.take() {
            net.stop();
        }
        let effects = self.runtime.set_enabled(false);
        self.host(effects, bus);
        true
    }

    fn open(&mut self, offer: u64, app: String, uri: Option<String>, bus: &Bus<'_, Self>) {
        let app = if app.is_empty() {
            self.listen.opener.clone()
        } else {
            app
        };
        let start = Start {
            app,
            role: Role::Ui,
            activation_token: None,
            uri,
            new_window: false,
        };
        let started = bus.later(move |client| {
            let ok = client.call(&start, CALL_TIME).is_ok_and(|r| r.opened);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                let effects = service.runtime.node.opened(offer, ok);
                let left = service.runtime.apply(effects);
                service.host(left, bus);
            })
        });
        if !started {
            let effects = self.runtime.node.opened(offer, false);
            let left = self.runtime.apply(effects);
            self.host(left, bus);
        }
    }

    /// Asks the person in the system dialog whether to take the offer. Where no dialog
    /// service answers, the offer stays open for the shell (`mesh.handoff_answer`) and
    /// expires by itself.
    fn ask(&mut self, incoming: HandoffIncoming, bus: &Bus<'_, Self>) {
        let offer = incoming.offer;
        let question = AskHandoff {
            from_name: incoming.from_name,
            from_kind: incoming.from_kind,
            title: incoming.title,
            app: incoming.app,
            size: incoming.size,
            stage: incoming.stage,
        };
        let _ = bus.later(move |client| {
            let wait = Duration::from_millis(sysdialog::CALL_TIMEOUT_MS);
            let decision = client.call(&question, wait).ok();
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                if let Some(decision) = decision {
                    let now = Instant::now();
                    let (_, effects) = service.runtime.node.answer(offer, decision.accept, now);
                    service.act(effects, bus);
                }
            })
        });
    }

    /// Does what is left for the host: tell the shell, open an app, publish the state.
    fn host(&mut self, effects: Vec<Effect>, bus: &Bus<'_, Self>) {
        let mut changed = false;
        for effect in effects {
            match effect {
                Effect::Incoming(incoming) => {
                    bus.publish(&incoming);
                    self.ask(incoming, bus);
                }
                Effect::Progress(progress) => bus.publish(&progress),
                Effect::Clipboard(text) => bus.publish(&text),
                Effect::Open { offer, app, uri } => self.open(offer, app, uri, bus),
                Effect::Changed => changed = true,
                other => self.model_effect(other, bus),
            }
        }
        if changed {
            self.publish(bus);
        }
    }

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.runtime.node.state();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    pub(crate) fn act(&mut self, effects: Vec<Effect>, bus: &Bus<'_, Self>) {
        let left = self.runtime.apply(effects);
        self.host(left, bus);
    }

    fn pairing(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let now = Instant::now();
        let method = request.method().to_owned();
        if method == PairBegin::METHOD {
            serve::<PairBegin>(request, |_, _| self.runtime.node.pair_begin(now));
        } else if method == PairJoin::METHOD {
            serve::<PairJoin>(request, |c, _| {
                let (ok, effects) = self.runtime.node.pair_join(&c.device, &c.code);
                self.act(effects, bus);
                Done { ok }
            });
        } else if method == Unpair::METHOD {
            serve::<Unpair>(request, |c, _| {
                let (ok, effects) = self.runtime.node.unpair(&c.device);
                self.act(effects, bus);
                Done { ok }
            });
        } else if method == SetRights::METHOD {
            serve::<SetRights>(request, |c, _| {
                let (ok, effects) = self.runtime.node.set_rights(&c.device, &c.rights);
                self.act(effects, bus);
                Done { ok }
            });
        } else {
            let _ = request.fail(ErrorCode::UnknownMethod);
        }
    }

    fn handing(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let now = Instant::now();
        let method = request.method().to_owned();
        if method == SetActivity::METHOD {
            serve::<SetActivity>(request, |c, caller| {
                // An app speaks for itself only; the shell names the app in front.
                let node = &mut self.runtime.node;
                let own = match ultar_svckit::access::app_name(caller) {
                    Some(app) => {
                        let own = c.activity.as_ref().is_none_or(|a| a.app == app);
                        if own {
                            node.report(app, c.activity);
                        }
                        own
                    }
                    None => {
                        node.set_front(c.activity);
                        true
                    }
                };
                Done { ok: own }
            });
        } else if method == HandoffTargets::METHOD {
            serve::<HandoffTargets>(request, |_, _| TargetList {
                targets: self.runtime.node.targets(),
            });
        } else if method == HandoffSend::METHOD {
            serve::<HandoffSend>(request, |c, _| {
                let (outcome, effects) = self.runtime.node.send(c.target.as_deref(), now);
                self.act(effects, bus);
                outcome
            });
        } else if method == SendFile::METHOD {
            serve::<SendFile>(request, |c, _| {
                let (outcome, effects) = self.runtime.node.send_file(&c.device, &c.path, now);
                self.act(effects, bus);
                outcome
            });
        } else if method == HandoffAnswer::METHOD {
            serve::<HandoffAnswer>(request, |c, _| {
                let (ok, effects) = self.runtime.node.answer(c.offer, c.accept, now);
                self.act(effects, bus);
                Done { ok }
            });
        } else {
            self.pairing(request, bus);
        }
    }
}

impl Service for MeshService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![LlmState::TOPIC]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if let Ok(state) = event.decode::<LlmState>() {
            self.model_changed(&state, bus);
        }
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        if let Ok(list) = bus.call(&List {}, CALL_TIME) {
            self.runtime
                .node
                .set_apps(list.apps.into_iter().map(|a| a.app).collect());
        }
        // What the network reports comes back to this loop.
        if let Some(inbox) = self.inbox.take() {
            let poster = bus.poster();
            let _ = std::thread::Builder::new().spawn(move || {
                while let Ok(event) = inbox.recv() {
                    let work = move |service: &mut Self, bus: &Bus<'_, Self>| {
                        let left = service.runtime.handle(event, Instant::now());
                        service.host(left, bus);
                    };
                    if !poster.post(Box::new(work)) {
                        break;
                    }
                }
            });
        }
        // Off until the person switches it on (the setting is kept by the caller).
        let effects = self.runtime.set_enabled(false);
        self.host(effects, bus);
        self.publish(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let Some(request) = self.lending(request, bus) else {
            return;
        };
        let Some(request) = self.remote(request, bus) else {
            return;
        };
        let method = request.method().to_owned();
        if method == SetEnabled::METHOD {
            serve::<SetEnabled>(request, |c, _| Done {
                ok: self.set_enabled(c.enabled, bus),
            });
        } else if method == ClipboardSet::METHOD {
            serve::<ClipboardSet>(request, |c, _| {
                let effects = self.runtime.node.clipboard_set(&c.text, c.confidential);
                self.act(effects, bus);
                Done { ok: true }
            });
        } else {
            self.handing(request, bus);
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        let now = Instant::now();
        self.expire_thinking(now);
        self.expire_acting(now);
        let left = self.runtime.tick(now);
        self.host(left, bus);
    }
}

/// A list of `a,b,c` as its parts.
pub fn list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The inbox below `home` when none is given.
pub fn default_inbox(home: &Path) -> PathBuf {
    home.join("Received")
}
