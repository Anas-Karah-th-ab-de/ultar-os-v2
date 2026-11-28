//! The bus side of the audio service.

use crate::engine::{Engine, Event};
use crate::mixer::{Mixer, Saved};
use crate::sound::{is_sound_call, Sound};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_bus::messages::audio::{
    AudioReply, AudioState, Outputs, OutputsReply, SetMute, SetOutput, SetVolume, SERVICE,
};
use ultar_bus::messages::settings::{Changed, Get, SettingValue};
use ultar_bus::messages::voice::{AudioDone, Capture, Play};
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_hal_linux::CardControls;
use ultar_service::log::{Level, Record};
use ultar_svckit::{store, Bus, Service};

/// How often the cards are looked at again (hot-plug, changes by other programs).
pub const TICK: Duration = Duration::from_secs(5);

/// The setting that puts both channels into every speaker.
const MONO_KEY: &str = "accessibility.mono_audio";

fn mono_of(value: &SettingValue) -> Option<bool> {
    match value {
        SettingValue::Flag(on) => Some(*on),
        _ => None,
    }
}

/// The audio service.
pub struct AudioService {
    mono_known: bool,
    mixer: Mixer,
    state_file: PathBuf,
    last: Option<AudioState>,
    sound: Option<Sound>,
}

impl AudioService {
    /// A service over `hal`, keeping its choices below `state_dir` (a damaged file is
    /// ignored: the driver defaults stay).
    pub fn new(hal: Box<dyn CardControls>, state_dir: PathBuf) -> Self {
        let state_file = state_dir.join("audio");
        let saved = store::load::<Saved>(&state_file).unwrap_or_default();
        Self {
            mono_known: false,
            mixer: Mixer::new(hal, saved),
            state_file,
            last: None,
            sound: None,
        }
    }

    /// Makes the service the sound server: it takes `engine` (the one holder of the card).
    pub fn with_sound(mut self, engine: Engine) -> Self {
        self.sound = Some(Sound::new(engine));
        self
    }

    /// An event of the engine thread (runs on the service loop).
    pub fn sound_event(&mut self, event: Event, bus: &Bus<'_, Self>) {
        if let Some(s) = self.sound.as_mut() {
            s.event(event, bus);
        }
    }

    fn sound_call(&mut self, request: IncomingRequest) {
        let method = request.method().to_owned();
        let caller = request.caller().to_owned();
        if method == Capture::METHOD {
            let Ok(c) = request.decode::<Capture>() else {
                let _ = request.fail(ErrorCode::BadPayload);
                return;
            };
            let ok = self
                .sound
                .as_mut()
                .is_some_and(|s| s.set_capture(&caller, c.on));
            let _ = request.reply(&AudioDone { ok });
        } else {
            let Ok(p) = request.decode::<Play>() else {
                let _ = request.fail(ErrorCode::BadPayload);
                return;
            };
            match self.sound.as_mut() {
                Some(s) => s.play(request, p),
                None => {
                    let _ = request.reply(&AudioDone { ok: false });
                }
            }
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
        if let Some(s) = self.sound.as_mut() {
            s.start(bus);
        }
        if self.mixer.state().present {
            bus.log(Record::new(Level::Info, "audiod.device"));
        } else {
            bus.log(Record::new(Level::Info, "audiod.no_device"));
        }
        self.publish(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        if is_sound_call(request.method()) {
            self.sound_call(request);
            return;
        }
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

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![Changed::TOPIC]
    }

    fn event(&mut self, event: RawEvent, _bus: &Bus<'_, Self>) {
        if event.topic != Changed::TOPIC {
            return;
        }
        if let Ok(c) = event.decode::<Changed>() {
            if c.key == MONO_KEY {
                if let (Some(on), Some(s)) = (mono_of(&c.value), self.sound.as_mut()) {
                    s.set_mono(on);
                }
                self.mono_known = true;
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        if let Some(s) = self.sound.as_mut() {
            s.expire(Instant::now());
        }
        if !self.mono_known && self.sound.is_some() {
            // until the settings service answers, mono is off
            let get = Get {
                key: MONO_KEY.to_owned(),
            };
            if let Ok(reply) = bus.call(&get, Duration::from_millis(200)) {
                self.mono_known = true;
                if let (Some(on), Some(s)) =
                    (reply.value.as_ref().and_then(mono_of), self.sound.as_mut())
                {
                    s.set_mono(on);
                }
            }
        }
        self.mixer.rescan();
        self.publish(bus);
    }
}
