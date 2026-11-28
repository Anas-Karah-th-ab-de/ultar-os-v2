//! The bus side of remote view: the viewer app's calls, the question to the person on the
//! viewed device, the hand-over to the screen service (`ultar-screend`) and the journal.
//!
//! The logic of a session is [`crate::view`]; this file only moves its effects onto the
//! bus: what the viewer app follows (`mesh.view`), what the screen service follows
//! (`mesh.view_serve`), the retained `mesh.view_state` of the shell, and one journal line
//! for every start, refusal and end.

use crate::node::Effect;
use crate::service::MeshService;
use crate::view::ViewEnd;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::Stage;
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_bus::messages::mesh_view::{
    EndReason, PictureTaken, RemoteInput, ServeEvent, ViewDone, ViewEvent, ViewInput, ViewLocked,
    ViewPicture, ViewReady, ViewStart, ViewStarted, ViewState, ViewStop,
};
use ultar_bus::messages::sysdialog;
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_service::log::{Level, Record};
use ultar_svckit::Bus;

/// Bus identity of the screen service: the only caller of the source-side calls.
pub const SCREEND: &str = "system.screend";
/// The shell: it ends a session on the viewed device (its Stop button).
pub const SHELL: &str = "system.shell";
/// How long the screen service has to answer a begun session before it is refused.
pub const READY_TIME: Duration = Duration::from_secs(8);

/// A session this device is asked to serve.
#[derive(Debug)]
struct Serving {
    peer: String,
    id: u64,
    /// When the screen service must have answered (`None` while the person is asked).
    deadline: Option<Instant>,
}

/// What the bus side remembers about remote view.
#[derive(Debug, Default)]
pub(crate) struct ViewHost {
    watching: Option<(String, u64)>,
    serving: Option<Serving>,
    last: Option<ViewState>,
}

fn reason(end: ViewEnd) -> EndReason {
    match end {
        ViewEnd::Refused => EndReason::Refused,
        ViewEnd::Stopped => EndReason::Stopped,
        ViewEnd::Lost => EndReason::Lost,
        ViewEnd::Locked => EndReason::Locked,
    }
}

fn word(end: ViewEnd) -> &'static str {
    match end {
        ViewEnd::Refused => "refused",
        ViewEnd::Stopped => "stopped",
        ViewEnd::Lost => "lost",
        ViewEnd::Locked => "locked",
    }
}

fn done(request: IncomingRequest, ok: bool) {
    let _ = request.reply(&ViewDone { ok });
}

impl MeshService {
    fn peer_name(&self, id: &str) -> String {
        let state = self.runtime.node.state();
        let found = state.peers.into_iter().find(|p| p.id == id);
        found.map_or_else(String::new, |p| p.name)
    }

    fn view_state(&mut self, bus: &Bus<'_, Self>) {
        let now = self.runtime.node.view_now();
        let state = ViewState {
            watched_by: now.watched_by.map(|id| self.peer_name(&id)),
            watching: now.watching.map(|id| self.peer_name(&id)),
        };
        if self.view.last.as_ref() != Some(&state) {
            bus.retain(&state);
            self.view.last = Some(state);
        }
    }

    /// The calls of remote view; gives the request back when it is another method.
    pub(crate) fn view_call(
        &mut self,
        request: IncomingRequest,
        bus: &Bus<'_, Self>,
    ) -> Option<IncomingRequest> {
        let method = request.method().to_owned();
        let from_screend = request.caller() == SCREEND;
        // Stop on the viewed device comes from the shell or, as the notification's button, screend.
        let from_shell = request.caller() == SHELL || from_screend;
        if method == ViewStart::METHOD {
            self.view_start(request, bus);
        } else if method == ViewInput::METHOD {
            let ok = match request.decode::<ViewInput>() {
                Ok(c) => self.runtime.node.view_input(&c.input.word()),
                Err(_) => return self.bad(request),
            };
            let sent = ok.is_some();
            self.act(ok.unwrap_or_default(), bus);
            done(request, sent);
        } else if method == ViewStop::METHOD {
            self.view_stop_call(request, from_shell, bus);
        } else if method == ViewReady::METHOD && from_screend {
            return self.view_ready_call(request, bus);
        } else if method == ViewPicture::METHOD && from_screend {
            return self.view_picture_call(request, bus);
        } else if method == ViewLocked::METHOD && from_screend {
            let effects = self.runtime.node.view_locked();
            let ended = !effects.is_empty();
            self.act(effects, bus);
            done(request, ended);
        } else if [ViewReady::METHOD, ViewPicture::METHOD, ViewLocked::METHOD]
            .contains(&method.as_str())
        {
            // Only the screen service speaks for the screen.
            let _ = request.fail(ErrorCode::UnknownMethod);
        } else {
            return Some(request);
        }
        None
    }

    /// The shell's Stop ends what this device serves first; an app ends what it watches.
    fn view_stop_call(&mut self, request: IncomingRequest, from_shell: bool, bus: &Bus<'_, Self>) {
        let serving = self.runtime.node.view_now().watched_by.is_some();
        let effects = if from_shell && serving {
            self.runtime.node.view_end()
        } else {
            self.runtime
                .node
                .view_stop()
                .unwrap_or_else(|| self.runtime.node.view_end())
        };
        let ended = !effects.is_empty();
        self.act(effects, bus);
        done(request, ended);
    }

    /// The screen service is ready: the screen has this size.
    fn view_ready_call(
        &mut self,
        request: IncomingRequest,
        bus: &Bus<'_, Self>,
    ) -> Option<IncomingRequest> {
        let Ok(c) = request.decode::<ViewReady>() else {
            return self.bad(request);
        };
        let peer = (self.view.serving.as_ref())
            .filter(|s| s.id == c.id)
            .map(|s| s.peer.clone());
        let effects = peer.map_or_else(Vec::new, |p| {
            self.runtime.node.view_accept(&p, c.id, c.width, c.height)
        });
        let ok = effects
            .iter()
            .any(|e| matches!(e, Effect::ViewStarted { .. }));
        self.act(effects, bus);
        done(request, ok);
        None
    }

    /// The screen service hands over one damaged rectangle.
    fn view_picture_call(
        &mut self,
        request: IncomingRequest,
        bus: &Bus<'_, Self>,
    ) -> Option<IncomingRequest> {
        let Ok(c) = request.decode::<ViewPicture>() else {
            return self.bad(request);
        };
        let effects = self.runtime.node.view_frame([c.x, c.y, c.w, c.h], &c.data);
        let taken = !effects.is_empty();
        self.act(effects, bus);
        let _ = request.reply(&PictureTaken { taken });
        None
    }

    fn bad(&mut self, request: IncomingRequest) -> Option<IncomingRequest> {
        let _ = request.fail(ErrorCode::BadPayload);
        None
    }

    fn view_start(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let Ok(call) = request.decode::<ViewStart>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        let Some((id, effects)) = self.runtime.node.view_start(&call.device) else {
            let _ = request.reply(&ViewStarted { ok: false });
            return;
        };
        self.view.watching = Some((call.device.clone(), id));
        let name = self.peer_name(&call.device);
        bus.publish(&ViewEvent::Waiting { name });
        bus.log(Record::new(Level::Info, "mesh.view_ask").id_or_invalid("peer", &call.device));
        self.act(effects, bus);
        self.view_state(bus);
        let _ = request.reply(&ViewStarted { ok: true });
    }

    /// Asks the person on this device whether `peer` may see and operate the screen.
    fn view_ask(&mut self, peer: String, id: u64, bus: &Bus<'_, Self>) {
        self.view.serving = Some(Serving {
            peer: peer.clone(),
            id,
            deadline: None,
        });
        // The dialog of the system has its own text for this stage ("<device> wants to see
        // and operate this screen", Allow / Deny).
        let found = self
            .runtime
            .node
            .state()
            .peers
            .into_iter()
            .find(|p| p.id == peer);
        let question = AskHandoff {
            from_name: found.as_ref().map_or_else(String::new, |p| p.name.clone()),
            from_kind: found.map_or_else(Default::default, |p| p.kind),
            title: String::new(),
            app: String::new(),
            size: None,
            stage: Stage::RemoteView,
        };
        let to = peer.clone();
        let started = bus.later(move |client| {
            let wait = Duration::from_millis(sysdialog::CALL_TIMEOUT_MS);
            let allowed = client.call(&question, wait).is_ok_and(|r| r.accept);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                service.view_decided(&to, id, allowed, bus);
            })
        });
        if !started {
            self.view_decided(&peer, id, false, bus);
        }
    }

    fn view_decided(&mut self, peer: &str, id: u64, allowed: bool, bus: &Bus<'_, Self>) {
        let waiting = self
            .view
            .serving
            .as_ref()
            .is_some_and(|s| s.peer == peer && s.id == id && s.deadline.is_none());
        if !waiting {
            return;
        }
        if !allowed {
            self.view.serving = None;
            let effects = self.runtime.node.view_refuse(peer, id);
            self.act(effects, bus);
            return;
        }
        if let Some(s) = self.view.serving.as_mut() {
            s.deadline = Some(Instant::now() + READY_TIME);
        }
        let viewer = self.peer_name(peer);
        bus.publish(&ServeEvent::Begin { id, viewer });
    }

    /// A session whose screen service never answered is refused.
    pub(crate) fn expire_view(&mut self, now: Instant, bus: &Bus<'_, Self>) {
        let late = self
            .view
            .serving
            .as_ref()
            .filter(|s| s.deadline.is_some_and(|d| now >= d))
            .map(|s| (s.peer.clone(), s.id));
        if let Some((peer, id)) = late {
            self.view.serving = None;
            bus.publish(&ServeEvent::End { id });
            let effects = self.runtime.node.view_refuse(&peer, id);
            self.act(effects, bus);
        }
    }

    /// The effects of remote view; everything else is not for this file.
    pub(crate) fn view_effect(&mut self, effect: Effect, bus: &Bus<'_, Self>) {
        match effect {
            Effect::ViewIncoming { peer, id } => {
                bus.log(
                    Record::new(Level::Info, "mesh.view_incoming").id_or_invalid("peer", &peer),
                );
                self.view_ask(peer, id, bus);
            }
            Effect::ViewStarted {
                peer,
                id,
                width,
                height,
            } => {
                let viewer = self
                    .view
                    .watching
                    .as_ref()
                    .is_some_and(|w| w.0 == peer && w.1 == id);
                if viewer {
                    bus.publish(&ViewEvent::Running { width, height });
                } else {
                    bus.log(
                        Record::new(Level::Info, "mesh.view_serving").id_or_invalid("peer", &peer),
                    );
                }
            }
            Effect::ViewFrame {
                region: [x, y, w, h],
                data,
                ..
            } => {
                bus.publish(&ViewEvent::Picture { x, y, w, h, data });
            }
            Effect::ViewInput { id, input, .. } => {
                if let Some(input) = RemoteInput::parse(&input) {
                    bus.publish(&ServeEvent::Input { id, input });
                }
            }
            Effect::ViewEnded { peer, id, end } => self.view_ended(&peer, id, end, bus),
            _ => return,
        }
        self.view_state(bus);
    }

    fn view_ended(&mut self, peer: &str, id: u64, end: ViewEnd, bus: &Bus<'_, Self>) {
        let record = |key: &'static str, end: ViewEnd| {
            Record::new(Level::Info, key)
                .id_or_invalid("peer", peer)
                .word("how", word(end))
        };
        if self
            .view
            .watching
            .as_ref()
            .is_some_and(|w| w.0 == peer && w.1 == id)
        {
            self.view.watching = None;
            bus.publish(&ViewEvent::Ended {
                reason: reason(end),
            });
            bus.log(record("mesh.view_end_viewer", end));
        }
        if self
            .view
            .serving
            .as_ref()
            .is_some_and(|s| s.peer == peer && s.id == id)
        {
            self.view.serving = None;
            bus.publish(&ServeEvent::End { id });
            bus.log(record("mesh.view_end_source", end));
        }
    }
}
