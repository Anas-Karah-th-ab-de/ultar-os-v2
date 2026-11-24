//! The bus side of the media service.

use crate::session::Session;
use std::time::{Duration, Instant};
use ultar_bus::messages::media::{
    Button, Command, MediaOutcome, MediaReply, Next, PlayPause, Previous, Release, Update, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::{Access, Bus, Service};

/// The media service.
pub struct MediaService {
    session: Session,
    access: Access,
    started: Instant,
}

impl MediaService {
    /// A service with `access` deciding who presses buttons (`media.system`).
    pub fn new(access: Access) -> Self {
        Self {
            session: Session::default(),
            access,
            started: Instant::now(),
        }
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn button(&mut self, request: IncomingRequest, button: Button, bus: &Bus<'_, Self>) {
        let outcome = if !self.access.is_system(request.caller()) {
            MediaOutcome::Unsupported
        } else {
            match self.session.press(button) {
                Ok(player) => {
                    bus.publish(&Command { player, button });
                    MediaOutcome::Done
                }
                Err(o) => o,
            }
        };
        let _ = request.reply(&MediaReply { outcome });
    }
}

impl Service for MediaService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(Duration::from_secs(5))
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        bus.retain(&self.session.state());
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let caller = request.caller().to_owned();
        let outcome = match request.method() {
            m if m == Update::METHOD => {
                let now = self.now();
                request
                    .decode::<Update>()
                    .map(|u| self.session.update(&caller, u, now))
            }
            m if m == Release::METHOD => request
                .decode::<Release>()
                .map(|_| self.session.release(&caller)),
            m if m == PlayPause::METHOD => return self.button(request, Button::PlayPause, bus),
            m if m == Next::METHOD => return self.button(request, Button::Next, bus),
            m if m == Previous::METHOD => return self.button(request, Button::Previous, bus),
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
                return;
            }
        };
        match outcome {
            Ok(outcome) => {
                let _ = request.reply(&MediaReply { outcome });
                bus.retain(&self.session.state());
            }
            Err(_) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        if self.session.expire(self.now()) {
            bus.retain(&self.session.state());
        }
    }
}
