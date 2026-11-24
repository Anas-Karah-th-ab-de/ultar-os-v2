//! The bus side of the notification service.

use crate::notes::{Notes, PostError};
use std::time::{SystemTime, UNIX_EPOCH};
use ultar_bus::messages::notify::{
    Dismiss, Dismissed, Invoke, InvokedHint, List, ListReply, NoteOutcome, NoteReply, Post,
    PostReply, TakeInvoked, TakeInvokedReply, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Access, Bus, Service};

/// The notification service.
pub struct NotifyService {
    notes: Notes,
    access: Access,
    clock: fn() -> u64,
}

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
        }
    }

    fn post(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let sender = request.caller().to_owned();
        let Ok(post) = request.decode::<Post>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        match self.notes.post(&sender, post, (self.clock)()) {
            Ok((posted, dropped)) => {
                let id = posted.id;
                bus.publish(&posted);
                for id in dropped {
                    bus.publish(&Dismissed { id });
                }
                let _ = request.reply(&PostReply { id });
            }
            Err(PostError::Invalid) => {
                bus.log(Record::new(Level::Warn, "notifyd.invalid"));
                let _ = request.fail(ErrorCode::BadPayload);
            }
            Err(PostError::TooMany) => {
                bus.log(Record::new(Level::Warn, "notifyd.too_many"));
                let _ = request.fail(ErrorCode::LimitExceeded);
            }
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

    fn start(&mut self, _bus: &Bus<'_, Self>) {}

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let caller = request.caller().to_owned();
        let system = self.access.is_system(&caller);
        match request.method() {
            m if m == Post::METHOD => self.post(request, bus),
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
