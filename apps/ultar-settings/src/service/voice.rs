//! Voice control behind the section on Settings > Assistant, as a port: over the bus
//! ([`BusVoice`]) and for tests ([`FakeVoice`]). The page never hears or sends audio: it
//! only switches the service on and off, chooses its language and model, asks it to record
//! the person's training takes itself and shows what the service reports.

use super::bus::{call, Retained};
use super::fake::Shared;
use super::{PortError, PortResult};
use std::time::Duration;
use ultar_bus::messages::voice::{
    Enroll, EnrollKind, Forget, Phase, SetEnabled, SetLanguage, SetModel, Test, TestResult,
    VoiceOutcome, VoiceReply, VoiceState, TAKES,
};
use ultar_bus::{Client, Text};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// What the section asks of the voice service.
pub trait VoicePort {
    /// What the service reports (`Err`: no voice service on this device).
    fn state(&mut self) -> PortResult<VoiceState>;
    /// Voice control on or off.
    fn set_enabled(&mut self, on: bool) -> PortResult<()>;
    /// The speech language (`ar`, `en`, `de`).
    fn set_language(&mut self, tag: &str) -> PortResult<()>;
    /// The speech-to-text model.
    fn set_model(&mut self, name: &str) -> PortResult<()>;
    /// Records the next training take of `kind`.
    fn enroll(&mut self, kind: EnrollKind) -> PortResult<()>;
    /// Forgets the training of `kind`.
    fn forget(&mut self, kind: EnrollKind) -> PortResult<()>;
    /// Waits once for the wake word and says whether it was heard.
    fn test(&mut self) -> PortResult<()>;
}

/// The message for what the service answered (`None`: it worked).
pub fn outcome_key(outcome: VoiceOutcome) -> Option<&'static str> {
    match outcome {
        VoiceOutcome::Done | VoiceOutcome::Nothing => None,
        VoiceOutcome::Disabled => Some("app.settings.voice.error.disabled"),
        VoiceOutcome::NotEnrolled => Some("app.settings.voice.error.not_enrolled"),
        VoiceOutcome::NoAudio => Some("app.settings.voice.error.no_audio"),
        VoiceOutcome::Busy => Some("app.settings.voice.error.busy"),
        VoiceOutcome::Invalid => Some("app.settings.voice.error.invalid"),
        VoiceOutcome::NotInstalled => Some("app.settings.voice.error.not_installed"),
    }
}

fn answered(reply: PortResult<VoiceReply>) -> PortResult<()> {
    match reply?.outcome {
        outcome if outcome_key(outcome).is_none() => Ok(()),
        outcome => Err(PortError::Refused(Text::key(
            outcome_key(outcome).unwrap_or("app.settings.error.failed"),
        ))),
    }
}

/// `voice` over the bus.
pub struct BusVoice {
    client: Client,
    state: Retained<VoiceState>,
}

impl std::fmt::Debug for BusVoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusVoice")
    }
}

impl BusVoice {
    /// Subscribes to `voice.state`.
    pub fn new(client: &Client) -> BusVoice {
        BusVoice {
            client: client.clone(),
            state: Retained::new(client),
        }
    }
}

impl VoicePort for BusVoice {
    fn state(&mut self) -> PortResult<VoiceState> {
        self.state.get()
    }

    fn set_enabled(&mut self, on: bool) -> PortResult<()> {
        answered(call(&self.client, &SetEnabled { on }, CALL_TIMEOUT))
    }

    fn set_language(&mut self, tag: &str) -> PortResult<()> {
        let tag = tag.to_owned();
        answered(call(&self.client, &SetLanguage { tag }, CALL_TIMEOUT))
    }

    fn set_model(&mut self, name: &str) -> PortResult<()> {
        let name = name.to_owned();
        answered(call(&self.client, &SetModel { name }, CALL_TIMEOUT))
    }

    fn enroll(&mut self, kind: EnrollKind) -> PortResult<()> {
        answered(call(&self.client, &Enroll { kind }, CALL_TIMEOUT))
    }

    fn forget(&mut self, kind: EnrollKind) -> PortResult<()> {
        answered(call(&self.client, &Forget { kind }, CALL_TIMEOUT))
    }

    fn test(&mut self) -> PortResult<()> {
        answered(call(&self.client, &Test {}, CALL_TIMEOUT))
    }
}

/// The voice service of the test world (`World::system.voice`). Training counts takes like
/// the real service; the test hears the word once a word is trained.
#[derive(Debug, Clone)]
pub struct FakeVoice(pub Shared);

impl FakeVoice {
    fn with<T>(&self, line: String, f: impl FnOnce(&mut VoiceState) -> T) -> PortResult<T> {
        let mut world = self.0.borrow_mut();
        world.log.push(line);
        let state = world.system.voice.as_mut().ok_or(PortError::Unavailable)?;
        Ok(f(state))
    }

    fn refuse(outcome: VoiceOutcome) -> PortResult<()> {
        answered(Ok(VoiceReply { outcome }))
    }
}

impl VoicePort for FakeVoice {
    fn state(&mut self) -> PortResult<VoiceState> {
        self.0
            .borrow()
            .system
            .voice
            .clone()
            .ok_or(PortError::Unavailable)
    }

    fn set_enabled(&mut self, on: bool) -> PortResult<()> {
        self.with(format!("voice.set_enabled {on}"), |s| {
            s.enabled = on;
            s.phase = if on { Phase::Listening } else { Phase::Idle };
        })
    }

    fn set_language(&mut self, tag: &str) -> PortResult<()> {
        if !["ar", "en", "de"].contains(&tag) {
            return Self::refuse(VoiceOutcome::Invalid);
        }
        self.with(format!("voice.set_language {tag}"), |s| {
            s.language = tag.to_owned();
        })
    }

    fn set_model(&mut self, name: &str) -> PortResult<()> {
        let known = self
            .state()?
            .models
            .iter()
            .find(|m| m.name == name)
            .cloned();
        match known {
            None => return Self::refuse(VoiceOutcome::Invalid),
            Some(m) if !m.installed => return Self::refuse(VoiceOutcome::NotInstalled),
            Some(_) => {}
        }
        self.with(format!("voice.set_model {name}"), |s| {
            s.model = name.to_owned();
        })
    }

    fn enroll(&mut self, kind: EnrollKind) -> PortResult<()> {
        self.with(format!("voice.enroll {kind:?}"), |s| {
            // the take comes in at once; the third one completes the training
            let (clips, done) = match kind {
                EnrollKind::Wake => (&mut s.wake.clips, &mut s.wake.enrolled),
                EnrollKind::Secret => (&mut s.secret.clips, &mut s.secret.enrolled),
                EnrollKind::Voice => (&mut s.voice.clips, &mut s.voice.enrolled),
            };
            *clips = (*clips + 1).min(TAKES);
            *done = *clips >= TAKES;
        })
    }

    fn forget(&mut self, kind: EnrollKind) -> PortResult<()> {
        self.with(format!("voice.forget {kind:?}"), |s| match kind {
            EnrollKind::Wake => (s.wake.clips, s.wake.enrolled) = (0, false),
            EnrollKind::Secret => (s.secret.clips, s.secret.enrolled) = (0, false),
            EnrollKind::Voice => (s.voice.clips, s.voice.enrolled) = (0, false),
        })
    }

    fn test(&mut self) -> PortResult<()> {
        let trained = {
            let s = self.state()?;
            s.wake.enrolled || s.secret.enrolled
        };
        if !trained {
            return Self::refuse(VoiceOutcome::NotEnrolled);
        }
        self.with("voice.test".to_owned(), |s| s.test = TestResult::Heard)
    }
}
