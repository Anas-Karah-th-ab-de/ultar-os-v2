//! The call service on the bus: registers `call`, answers every request of the contract
//! (`ultar_bus::messages::call`) through the [`Session`] and publishes `call.state`
//! (retained) whenever it changed. Lines are polled every `tick` while waiting for requests.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::emergency;
use crate::mono;
use crate::session::Session;
use std::time::Duration;
use ultar_bus::messages::call::{
    self as contract, Answer, Block, CallError, CallState, ClearLog, Decline, Dial, DialReply,
    Done, Dtmf, Emergency, Forget, Hangup, Hold, Log, LogReply, Mute, NextCamera, SendText,
    SetOutput, SetVideo, Share, Snapshot, Way,
};
use ultar_bus::messages::settings::{Changed, Get};
use ultar_bus::{BusError, Client, ErrorCode, IncomingRequest, ServiceEndpoint, TypedSubscription};

/// Poll interval of the lines while no request comes.
pub const DEFAULT_TICK: Duration = Duration::from_millis(50);

/// The running service.
pub struct Service {
    session: Session,
    bus: Client,
    endpoint: ServiceEndpoint,
    tick: Duration,
    published: Option<CallState>,
    /// `emergency.number`, validated (the default until the settings service answers).
    emergency_number: String,
    /// The settings service answered at least once.
    settings_known: bool,
    /// `settings.changed`.
    settings_changes: Option<TypedSubscription<Changed>>,
    /// Throttle of the settings read until it answers (ticks).
    settings_try: u32,
    /// `accessibility.mono_audio` and the kernel's switch it goes to.
    mono: mono::Switch,
    /// The settings service answered for `accessibility.mono_audio`.
    mono_known: bool,
}

fn code(e: CallError) -> ErrorCode {
    ErrorCode::Service(e.code())
}

/// Dials the emergency number on the mobile network: slot 1, else slot 2.
fn emergency_dial(s: &mut Session, number: &str) -> Result<u32, CallError> {
    let dial = |slot: u8| Dial {
        way: Way::Cellular(slot),
        number: number.to_owned(),
        video: false,
    };
    s.dial(&dial(1)).or_else(|_| s.dial(&dial(2)))
}

/// Two states differ in something other than the clock.
fn differs(a: &CallState, b: &CallState) -> bool {
    let mut a = a.clone();
    a.clock_ms = b.clock_ms;
    a != *b
}

impl Service {
    /// Registers the service `call` on `bus`.
    pub fn new(bus: Client, session: Session, tick: Duration) -> Result<Service, BusError> {
        let endpoint = bus.register(contract::SERVICE)?;
        let settings_changes = bus.subscribe::<Changed>().ok();
        Ok(Service {
            session,
            bus,
            endpoint,
            tick,
            published: None,
            emergency_number: emergency::DEFAULT.to_owned(),
            settings_known: false,
            settings_changes,
            settings_try: 0,
            mono: mono::Switch::new(ultar_hal_system::seat_nodes::pcm_mono_node(
                std::path::Path::new("/"),
            )),
            mono_known: false,
        })
    }

    /// The kernel's mono switch at `node` (tests: a file).
    pub fn with_mono_node(mut self, node: std::path::PathBuf) -> Self {
        self.mono = mono::Switch::new(node);
        self
    }

    /// The session (tests read the state through it).
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Publishes the state when it changed since the last publication.
    pub fn publish_if_changed(&mut self) -> Result<bool, BusError> {
        let state = self.session.state();
        if self
            .published
            .as_ref()
            .is_some_and(|old| !differs(old, &state))
        {
            return Ok(false);
        }
        self.bus.publish(&state)?;
        self.published = Some(state);
        Ok(true)
    }

    /// One step: wait up to one tick for a request, answer it, poll the lines, publish.
    pub fn step(&mut self) -> Result<(), BusError> {
        self.follow_settings();
        match self.endpoint.recv_timeout(self.tick) {
            Ok(request) => self.handle(request),
            Err(BusError::Timeout) => {}
            Err(e) => return Err(e),
        }
        self.session.pump();
        self.publish_if_changed()?;
        Ok(())
    }

    /// `emergency.number` and `accessibility.mono_audio`: read once at start (again until
    /// the settings service answers), then follow every change. Until then the default 112
    /// is in effect and mono is off.
    fn follow_settings(&mut self) {
        if self.settings_changes.is_none() {
            self.settings_changes = self.bus.subscribe::<Changed>().ok();
        }
        while let Some(Ok(Some(c))) = self.settings_changes.as_ref().map(|s| s.try_recv()) {
            if c.key == emergency::KEY {
                self.emergency_number = emergency::of(&c.value);
                self.settings_known = true;
            } else if c.key == mono::KEY {
                if let Some(on) = mono::of(&c.value) {
                    self.mono.want(on);
                }
                self.mono_known = true;
            }
        }
        self.settings_try = self.settings_try.wrapping_add(1);
        // About every two seconds (40 ticks of 50 ms), not every tick.
        if self.settings_try % 40 == 1 {
            self.read_settings();
            // a sound card that came later, or a write that failed
            self.mono.apply();
        }
    }

    /// Asks the settings service for what it has not answered yet.
    fn read_settings(&mut self) {
        let read = |key: &str| {
            let get = Get {
                key: key.to_owned(),
            };
            self.bus.call(&get, Duration::from_millis(200)).ok()
        };
        if !self.settings_known {
            if let Some(reply) = read(emergency::KEY) {
                self.settings_known = true;
                if let Some(value) = reply.value {
                    self.emergency_number = emergency::of(&value);
                }
            }
        }
        if !self.mono_known {
            if let Some(reply) = read(mono::KEY) {
                self.mono_known = true;
                if let Some(on) = reply.value.as_ref().and_then(mono::of) {
                    self.mono.want(on);
                }
            }
        }
    }

    /// Runs until the bus connection ends.
    pub fn run(mut self) -> Result<(), BusError> {
        self.publish_if_changed()?;
        loop {
            self.step()?;
        }
    }

    fn handle(&mut self, request: IncomingRequest) {
        let s = &mut self.session;
        let done = |r: Result<(), CallError>| r.map(|()| Done).map_err(code);
        let outcome = match request.method() {
            "dial" => {
                request.respond::<Dial>(|d| s.dial(&d).map(|id| DialReply { id }).map_err(code))
            }
            // The lock screen's emergency call: the number is this service's own
            // (read from the settings, validated); the request carries none.
            "emergency" => {
                let number = self.emergency_number.clone();
                request.respond::<Emergency>(|_| {
                    emergency_dial(s, &number)
                        .map(|id| DialReply { id })
                        .map_err(code)
                })
            }
            "answer" => request.respond::<Answer>(|a| done(s.answer(a.id, a.others))),
            "decline" => request.respond::<Decline>(|d| done(s.decline(d.id))),
            "hangup" => request.respond::<Hangup>(|h| done(s.hangup(h.id))),
            "hold" => request.respond::<Hold>(|h| done(s.hold(h.id, h.on))),
            "mute" => request.respond::<Mute>(|m| done(s.mute(m.on))),
            "output" => request.respond::<SetOutput>(|o| done(s.set_output(o.output))),
            "dtmf" => request.respond::<Dtmf>(|d| done(s.dtmf(d.id, d.key))),
            "video" => request.respond::<SetVideo>(|v| done(s.set_video(v.id, v.on))),
            "camera" => request.respond::<NextCamera>(|c| done(s.next_camera(c.id))),
            "share" => request.respond::<Share>(|v| done(s.share(v.id, v.on))),
            "text" => request.respond::<SendText>(|t| done(s.send_text(&t))),
            "log" => request.respond::<Log>(|l| {
                Ok(LogReply {
                    entries: s.log(l.missed_only),
                })
            }),
            "forget" => request.respond::<Forget>(|f| done(s.forget(f.id))),
            "clear" => request.respond::<ClearLog>(|_| {
                s.clear_log();
                Ok(Done)
            }),
            "block" => request.respond::<Block>(|b| done(s.block(&b.number, b.on))),
            "snapshot" => request.respond::<Snapshot>(|_| Ok(s.state())),
            _ => request.fail(ErrorCode::UnknownMethod),
        };
        // A failed answer concerns one request only (caller gone, hostile payload answered
        // with `BadPayload` by the bus); the service goes on.
        let _ = outcome;
    }
}
