//! The bus side of the media service.

use crate::session::Session;
use crate::there::Sink;
use crate::there_service::ThereRt;
use std::time::{Duration, Instant};
use ultar_bus::messages::media::{
    Button, Command, MediaOutcome, MediaReply, Next, PlayPause, Previous, Release, Update, SERVICE,
};
use ultar_bus::messages::mesh::MeshState;
use ultar_bus::messages::mesh_media::{
    MediaControl, MediaIncoming, MediaPcmIn, MediaProgress, SetThere, TherePause, TherePush,
    ThereVolume,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_svckit::{Access, Bus, Service};

/// The media service.
pub struct MediaService {
    pub(crate) session: Session,
    pub(crate) access: Access,
    started: Instant,
    pub(crate) there: ThereRt,
}

impl MediaService {
    /// A service with `access` deciding who presses buttons (`media.system`).
    pub fn new(access: Access) -> Self {
        Self {
            session: Session::default(),
            access,
            started: Instant::now(),
            there: ThereRt::new(None),
        }
    }

    /// Lets this device play the sound of another device's media session on `sink`
    /// ("play there", the sink side).
    pub fn with_sink(mut self, sink: Box<dyn Sink>) -> Self {
        self.there = ThereRt::new(Some(sink));
        self
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

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![
            MeshState::TOPIC,
            MediaIncoming::TOPIC,
            MediaPcmIn::TOPIC,
            MediaControl::TOPIC,
            MediaProgress::TOPIC,
        ]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(Duration::from_secs(5))
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        bus.retain(&self.session.state());
        self.there_start(bus);
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        self.there_event(&event, bus);
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
            m if m == SetThere::METHOD
                || m == ThereVolume::METHOD
                || m == TherePause::METHOD
                || m == TherePush::METHOD =>
            {
                return self.there_request(request, bus)
            }
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
