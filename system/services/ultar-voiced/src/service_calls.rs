//! The calls of the person's surfaces (`voice.*`) and the enrolment takes.

use super::{slot, VoiceService, TEST_WALL};
use crate::detect::{TakeError, VoicePrint, WakeWord};
use crate::state::LANGUAGES;
use std::time::Instant;
use ultar_bus::messages::voice::{
    Enroll, EnrollKind, Forget, OwnerConfirmed, Phase, SetEnabled, SetLanguage, SetLocked,
    SetModel, Test, TestResult, VoiceError, VoiceOutcome, VoiceReply, TAKES,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::Bus;

impl VoiceService {
    fn reply(request: IncomingRequest, outcome: VoiceOutcome) {
        let _ = request.reply(&VoiceReply { outcome });
    }

    pub(super) fn calls(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let method = request.method().to_owned();
        let outcome = match method.as_str() {
            m if m == SetEnabled::METHOD => request
                .decode::<SetEnabled>()
                .map(|c| self.set_enabled(c.on, bus)),
            m if m == SetLanguage::METHOD => request
                .decode::<SetLanguage>()
                .map(|c| self.set_language(&c.tag, bus)),
            m if m == SetModel::METHOD => request
                .decode::<SetModel>()
                .map(|c| self.set_model(&c.name, bus)),
            m if m == Enroll::METHOD => request
                .decode::<Enroll>()
                .map(|c| self.begin_enrol(c.kind, bus)),
            m if m == Forget::METHOD => {
                request.decode::<Forget>().map(|c| self.forget(c.kind, bus))
            }
            m if m == Test::METHOD => request.decode::<Test>().map(|_| self.begin_test(bus)),
            m if m == SetLocked::METHOD => request
                .decode::<SetLocked>()
                .map(|c| self.set_locked(c.locked, bus)),
            m if m == OwnerConfirmed::METHOD => request
                .decode::<OwnerConfirmed>()
                .map(|_| self.owner_confirmed(bus)),
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
                return;
            }
        };
        match outcome {
            Ok(outcome) => Self::reply(request, outcome),
            Err(_) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn set_enabled(&mut self, on: bool, bus: &Bus<'_, Self>) -> VoiceOutcome {
        if self.settings.enabled != on {
            self.settings.enabled = on;
            self.save_settings(bus);
        }
        self.errors.job = None;
        self.apply(bus);
        VoiceOutcome::Done
    }

    fn set_language(&mut self, tag: &str, bus: &Bus<'_, Self>) -> VoiceOutcome {
        if !LANGUAGES.contains(&tag) {
            return VoiceOutcome::Invalid;
        }
        self.settings.language = tag.to_owned();
        self.save_settings(bus);
        self.publish(bus);
        VoiceOutcome::Done
    }

    fn set_model(&mut self, name: &str, bus: &Bus<'_, Self>) -> VoiceOutcome {
        if self.models.file(name).is_none() {
            return VoiceOutcome::Invalid;
        }
        if !self.models.installed(name) {
            return VoiceOutcome::NotInstalled;
        }
        self.settings.model = name.to_owned();
        self.save_settings(bus);
        self.apply(bus);
        VoiceOutcome::Done
    }

    fn quiet(&self) -> bool {
        matches!(self.pipe.phase(), Phase::Idle | Phase::Listening) && !self.busy
    }

    fn begin_enrol(&mut self, kind: EnrollKind, bus: &Bus<'_, Self>) -> VoiceOutcome {
        if !self.quiet() || self.test.result == TestResult::Waiting {
            return VoiceOutcome::Busy;
        }
        self.errors.take = None;
        self.pipe.begin_enrol(kind);
        self.sync_capture(bus);
        self.publish(bus);
        VoiceOutcome::Done
    }

    fn forget(&mut self, kind: EnrollKind, bus: &Bus<'_, Self>) -> VoiceOutcome {
        self.pending[slot(kind)].clear();
        match kind {
            EnrollKind::Wake => self.enrol.wake = None,
            EnrollKind::Secret => self.enrol.secret = None,
            EnrollKind::Voice => self.enrol.print = None,
        }
        self.save_enrollment(bus);
        self.errors.take = None;
        self.apply(bus);
        VoiceOutcome::Done
    }

    fn begin_test(&mut self, bus: &Bus<'_, Self>) -> VoiceOutcome {
        if !self.quiet() || self.test.result == TestResult::Waiting {
            return VoiceOutcome::Busy;
        }
        if !self.has_wake_detector() {
            return VoiceOutcome::NotEnrolled;
        }
        self.errors.take = None;
        self.test.result = TestResult::Waiting;
        self.test.until = Some(Instant::now() + TEST_WALL);
        self.pipe.set_locked(self.locked);
        self.pipe.begin_test();
        self.sync_capture(bus);
        self.publish(bus);
        VoiceOutcome::Done
    }

    fn set_locked(&mut self, locked: bool, bus: &Bus<'_, Self>) -> VoiceOutcome {
        self.locked = locked;
        self.apply(bus);
        VoiceOutcome::Done
    }

    fn owner_confirmed(&mut self, bus: &Bus<'_, Self>) -> VoiceOutcome {
        if self.verify_until.take().is_none() {
            return VoiceOutcome::Nothing;
        }
        self.pipe.accept_next_speaker();
        self.publish(bus);
        VoiceOutcome::Done
    }

    /// One enrolment take is complete.
    pub(super) fn take(&mut self, kind: EnrollKind, samples: Vec<f32>, bus: &Bus<'_, Self>) {
        let i = slot(kind);
        let mfcc = self.pipe.mfcc();
        if kind != EnrollKind::Voice {
            if let Err(e) = WakeWord::accepts_take(mfcc, &self.pending[i], &samples) {
                self.errors.take = Some(match e {
                    TakeError::TooShort => VoiceError::TakeShort,
                    TakeError::Differs => VoiceError::TakeDiffers,
                });
                return;
            }
        }
        self.pending[i].push(samples);
        if self.pending[i].len() < usize::from(TAKES) {
            return;
        }
        let takes = std::mem::take(&mut self.pending[i]);
        let mfcc = self.pipe.mfcc();
        let built = match kind {
            EnrollKind::Voice => {
                VoicePrint::from_takes(mfcc, &takes).map(|p| self.enrol.print = Some(p))
            }
            _ => WakeWord::from_takes(mfcc, &takes).map(|w| match kind {
                EnrollKind::Wake => self.enrol.wake = Some(w),
                _ => self.enrol.secret = Some(w),
            }),
        };
        match built {
            Ok(()) => {
                self.save_enrollment(bus);
                self.pipe.enrol(
                    self.enrol.wake.clone(),
                    self.enrol.secret.clone(),
                    self.enrol.print.clone(),
                );
            }
            Err(e) => {
                self.errors.take = Some(match e {
                    TakeError::TooShort => VoiceError::TakeShort,
                    TakeError::Differs => VoiceError::TakeDiffers,
                })
            }
        }
    }
}
