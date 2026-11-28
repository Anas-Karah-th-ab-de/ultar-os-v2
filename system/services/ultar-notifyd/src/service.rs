//! The bus side of the notification service.

use crate::mirror;
use crate::notes::{Notes, PostError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ultar_bus::messages::mesh_calls::NoteRemote;
use ultar_bus::messages::notify::{
    Dismiss, Dismissed, Invoke, InvokedHint, List, ListReply, NoteOutcome, NoteReply, Post,
    PostReply, TakeInvoked, TakeInvokedReply, SERVICE,
};
use ultar_bus::messages::notify_mirror::{PostEx, Privacy};
use ultar_bus::messages::session::SessionState;
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Access, Bus, Service};

/// The notification service.
pub struct NotifyService {
    notes: Notes,
    access: Access,
    clock: fn() -> u64,
    /// Who is signed in (`session.state`); until it is known the device counts as locked.
    session: Option<SessionState>,
}

/// How long the mesh service may take to take one mirrored notification.
const MIRROR_WAIT: Duration = Duration::from_millis(300);

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl NotifyService {
    /// A service with `access` deciding who is the shell (`notify.system`).
    pub fn new(access: Access) -> Self {
        Self {
            notes: Notes::default(),
            access,
            clock: wall_ms,
            session: None,
        }
    }

    /// Nobody is signed in (the sign-in screen), or not known yet: a device that does not
    /// know is treated as locked, so that nothing private is shown by mistake.
    fn locked(&self) -> bool {
        self.session.as_ref().is_none_or(|s| s.user.is_none())
    }

    /// Takes a post and publishes it; the id, or the error to answer with.
    fn accept(&mut self, sender: &str, post: Post, bus: &Bus<'_, Self>) -> Result<u64, ErrorCode> {
        match self.notes.post(sender, post, (self.clock)()) {
            Ok((posted, dropped)) => {
                let id = posted.id;
                bus.publish(&posted);
                for id in dropped {
                    bus.publish(&Dismissed { id });
                }
                Ok(id)
            }
            Err(PostError::Invalid) => {
                bus.log(Record::new(Level::Warn, "notifyd.invalid"));
                Err(ErrorCode::BadPayload)
            }
            Err(PostError::TooMany) => {
                bus.log(Record::new(Level::Warn, "notifyd.too_many"));
                Err(ErrorCode::LimitExceeded)
            }
        }
    }

    fn post(&mut self, request: IncomingRequest, extended: bool, bus: &Bus<'_, Self>) {
        let sender = request.caller().to_owned();
        let (post, privacy) = if extended {
            match request.decode::<PostEx>() {
                Ok(c) => (c.post, c.privacy),
                Err(_) => {
                    let _ = request.fail(ErrorCode::BadPayload);
                    return;
                }
            }
        } else {
            match request.decode::<Post>() {
                Ok(post) => (post, Privacy::Normal),
                Err(_) => {
                    let _ = request.fail(ErrorCode::BadPayload);
                    return;
                }
            }
        };
        let mirror = mirror::outgoing(&post, privacy);
        match self.accept(&sender, post, bus) {
            Ok(id) => {
                // The person's other devices get what may leave this device; a missing or
                // switched-off mesh service is no error of the sender.
                if let Some(note) = mirror {
                    let _ = bus.later(move |client| {
                        let _ = client.call(&note, MIRROR_WAIT);
                        Box::new(|_: &mut Self, _: &Bus<'_, Self>| {})
                    });
                }
                let _ = request.reply(&PostReply { id });
            }
            Err(code) => {
                let _ = request.fail(code);
            }
        }
    }

    /// A notification of another device (`mesh.note`), shown with that device named.
    fn remote(&mut self, note: &NoteRemote, bus: &Bus<'_, Self>) {
        if let Some(post) = mirror::incoming(note, self.locked()) {
            let _ = self.accept(mirror::SENDER, post, bus);
        }
    }

    fn invoke(&mut self, request: IncomingRequest, system: bool, bus: &Bus<'_, Self>) {
        let Ok(call) = request.decode::<Invoke>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        let outcome = if !system {
            NoteOutcome::Refused
        } else {
            match self.notes.invoke(call.id, &call.action) {
                Ok(sender) => {
                    bus.publish(&Dismissed { id: call.id });
                    bus.publish(&InvokedHint { sender });
                    NoteOutcome::Done
                }
                Err(outcome) => outcome,
            }
        };
        let _ = request.reply(&NoteReply { outcome });
    }

    fn dismiss(&mut self, request: IncomingRequest, system: bool, bus: &Bus<'_, Self>) {
        let Ok(call) = request.decode::<Dismiss>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        let outcome = self.notes.dismiss(call.id, request.caller(), system);
        if outcome == NoteOutcome::Done {
            bus.publish(&Dismissed { id: call.id });
        }
        let _ = request.reply(&NoteReply { outcome });
    }
}

impl Service for NotifyService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![NoteRemote::TOPIC, SessionState::TOPIC]
    }

    fn start(&mut self, _bus: &Bus<'_, Self>) {}

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if event.topic == NoteRemote::TOPIC {
            if let Ok(note) = event.decode::<NoteRemote>() {
                self.remote(&note, bus);
            }
        } else if let Ok(state) = event.decode::<SessionState>() {
            self.session = Some(state);
        }
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let caller = request.caller().to_owned();
        let system = self.access.is_system(&caller);
        match request.method() {
            m if m == Post::METHOD => self.post(request, false, bus),
            m if m == PostEx::METHOD => self.post(request, true, bus),
            m if m == Invoke::METHOD => self.invoke(request, system, bus),
            m if m == Dismiss::METHOD => self.dismiss(request, system, bus),
            m if m == List::METHOD => {
                if system {
                    let notes = self.notes.list();
                    let _ = request.respond::<List>(|_| Ok(ListReply { notes }));
                } else {
                    let _ = request.fail(ErrorCode::PermissionDenied);
                }
            }
            m if m == TakeInvoked::METHOD => {
                let invoked = self.notes.take(&caller);
                let _ = request.respond::<TakeInvoked>(|_| Ok(TakeInvokedReply { invoked }));
            }
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
            }
        }
    }
}
