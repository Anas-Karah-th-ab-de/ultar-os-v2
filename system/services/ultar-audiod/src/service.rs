//! The bus side of the audio service.

use crate::mixer::{Mixer, Saved};
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::audio::{
    AudioReply, AudioState, Outputs, OutputsReply, SetMute, SetOutput, SetVolume, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_hal_linux::CardControls;
use ultar_service::log::{Level, Record};
use ultar_svckit::{store, Bus, Service};

/// How often the cards are looked at again (hot-plug, changes by other programs).
pub const TICK: Duration = Duration::from_secs(5);

/// The audio service.
pub struct AudioService {
    mixer: Mixer,
    state_file: PathBuf,
    last: Option<AudioState>,
}

impl AudioService {
    /// A service over `hal`, keeping its choices below `state_dir` (a damaged file is
    /// ignored: the driver defaults stay).
    pub fn new(hal: Box<dyn CardControls>, state_dir: PathBuf) -> Self {
        let state_file = state_dir.join("audio");
        let saved = store::load::<Saved>(&state_file).unwrap_or_default();
        Self {
            mixer: Mixer::new(hal, saved),
            state_file,
            last: None,
        }
    }

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.mixer.state();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    fn save(&self, bus: &Bus<'_, Self>) {
        if store::save(&self.state_file, self.mixer.saved()).is_err() {
            bus.log(Record::new(Level::Warn, "audiod.not_saved"));
        }
    }
}

impl Service for AudioService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        self.mixer.rescan();
        if self.mixer.state().present {
            bus.log(Record::new(Level::Info, "audiod.device"));
        } else {
            bus.log(Record::new(Level::Info, "audiod.no_device"));
        }
        self.publish(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let outcome = match request.method() {
            m if m == Outputs::METHOD => {
                let outputs = self.mixer.outputs();
                let _ = request.respond::<Outputs>(|_| Ok(OutputsReply { outputs }));
                return;
            }
            m if m == SetVolume::METHOD => request
                .decode::<SetVolume>()
                .map(|c| self.mixer.set_volume(c.percent)),
            m if m == SetMute::METHOD => request
                .decode::<SetMute>()
                .map(|c| self.mixer.set_mute(c.on)),
            m if m == SetOutput::METHOD => request
                .decode::<SetOutput>()
                .map(|c| self.mixer.set_output(&c.id)),
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
                return;
            }
        };
        match outcome {
            Ok(outcome) => {
                // Kept before it is confirmed: a crash right after the answer loses nothing.
                self.save(bus);
                let _ = request.reply(&AudioReply { outcome });
                self.publish(bus);
            }
            Err(_) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.mixer.rescan();
        self.publish(bus);
    }
}
