//! The call service on the bus: registers `call`, answers every request of the contract
//! (`ultar_bus::messages::call`) through the [`Session`] and publishes `call.state`
//! (retained) whenever it changed. Lines are polled every `tick` while waiting for requests.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::session::Session;
use std::time::Duration;
use ultar_bus::messages::call::{
    self as contract, Answer, Block, CallError, CallState, ClearLog, Decline, Dial, DialReply,
    Done, Dtmf, Forget, Hangup, Hold, Log, LogReply, Mute, NextCamera, SendText, SetOutput,
    SetVideo, Share, Snapshot,
};
use ultar_bus::{BusError, Client, ErrorCode, IncomingRequest, ServiceEndpoint};

/// Poll interval of the lines while no request comes.
pub const DEFAULT_TICK: Duration = Duration::from_millis(50);

/// The running service.
pub struct Service {
    session: Session,
    bus: Client,
    endpoint: ServiceEndpoint,
    tick: Duration,
    published: Option<CallState>,
}

fn code(e: CallError) -> ErrorCode {
    ErrorCode::Service(e.code())
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
        Ok(Service {
            session,
            bus,
            endpoint,
            tick,
            published: None,
        })
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
        match self.endpoint.recv_timeout(self.tick) {
            Ok(request) => self.handle(request),
            Err(BusError::Timeout) => {}
            Err(e) => return Err(e),
        }
        self.session.pump();
        self.publish_if_changed()?;
        Ok(())
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
