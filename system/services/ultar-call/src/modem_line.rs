//! [`Line`] for the mobile network over the modem service on the bus (`modem.*`, served by
//! `ultar-ril --daemon` with `ULTAR_BUS`). The modem's call list (`modem.state`) is turned
//! into line events: new incoming calls, alerting, connected, gone (= ended by the other side
//! or the network). Video, camera, sharing and texts are not part of a mobile voice call.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::line::{Key, LineError, LineEvent, Status};
use crate::Line;
use std::collections::HashMap;
use std::time::Duration;
use ultar_bus::messages::call::{End, Failure, Media, Way};
use ultar_bus::messages::modem::{
    ModemAnswer, ModemCallStatus, ModemDial, ModemDtmf, ModemHold, ModemMute, ModemRelease,
    ModemState, ERR_OFFLINE,
};
use ultar_bus::{BusError, Call, Client, ErrorCode, TypedSubscription};

/// Keys of outgoing calls start here (incoming calls use the modem index 1–7).
const OUTGOING_BASE: Key = 1000;

/// The mobile network through the modem service.
pub struct ModemLine {
    bus: Client,
    states: TypedSubscription<ModemState>,
    timeout: Duration,
    slot: u8,
    /// Modem index → (key, last status).
    known: HashMap<u8, (Key, ModemCallStatus)>,
    /// Outgoing calls dialled but not yet in the modem's list.
    pending: Vec<Key>,
    next_key: Key,
    last: Option<ModemState>,
}

fn line_err(e: BusError) -> LineError {
    match e {
        BusError::Remote(ErrorCode::Service(ERR_OFFLINE)) => {
            LineError::Unavailable(Failure::NoNetwork)
        }
        BusError::Remote(ErrorCode::Service(_)) => LineError::Unavailable(Failure::Rejected),
        _ => LineError::Backend,
    }
}

impl ModemLine {
    /// A line for SIM `slot` over `bus`; every command waits at most `timeout`.
    pub fn new(bus: Client, slot: u8, timeout: Duration) -> Result<ModemLine, BusError> {
        let states = bus.subscribe::<ModemState>()?;
        Ok(ModemLine {
            bus,
            states,
            timeout,
            slot,
            known: HashMap::new(),
            pending: Vec::new(),
            next_key: OUTGOING_BASE,
            last: None,
        })
    }

    fn call<C: Call>(&self, request: &C) -> Result<(), LineError> {
        self.bus
            .call(request, self.timeout)
            .map(|_| ())
            .map_err(line_err)
    }

    fn index_of(&self, key: Key) -> Result<u8, LineError> {
        self.known
            .iter()
            .find(|(_, (k, _))| *k == key)
            .map(|(i, _)| *i)
            .ok_or(LineError::UnknownCall)
    }

    /// Line events for a new modem state.
    fn diff(&mut self, state: &ModemState, out: &mut Vec<LineEvent>) {
        let status_changed = self
            .last
            .as_ref()
            .is_none_or(|l| l.cellular != state.cellular || l.sims != state.sims);
        if status_changed {
            let sims = state.sims.iter().map(|_| self.slot).collect();
            out.push(LineEvent::Status(Status::Cellular(state.cellular, sims)));
        }
        let way = Way::Cellular(self.slot);
        for call in &state.calls {
            match self.known.get(&call.index).copied() {
                None if call.incoming => {
                    let key = Key::from(call.index);
                    self.known.insert(call.index, (key, call.status));
                    let number = call.number.clone();
                    out.push(LineEvent::Incoming {
                        key,
                        way,
                        number,
                        name: None,
                        video: false,
                    });
                }
                None => {
                    let key = if self.pending.is_empty() {
                        Key::from(call.index)
                    } else {
                        self.pending.remove(0)
                    };
                    self.known.insert(call.index, (key, call.status));
                    self.progress(key, ModemCallStatus::Dialing, call.status, out);
                }
                Some((key, old)) => {
                    self.known.insert(call.index, (key, call.status));
                    self.progress(key, old, call.status, out);
                }
            }
        }
        let gone: Vec<u8> = self
            .known
            .keys()
            .filter(|i| !state.calls.iter().any(|c| c.index == **i))
            .copied()
            .collect();
        for index in gone {
            if let Some((key, _)) = self.known.remove(&index) {
                out.push(LineEvent::Ended(key, End::Remote));
            }
        }
        self.last = Some(state.clone());
    }

    fn progress(
        &self,
        key: Key,
        old: ModemCallStatus,
        new: ModemCallStatus,
        out: &mut Vec<LineEvent>,
    ) {
        use ModemCallStatus as S;
        match (old, new) {
            (S::Dialing, S::Alerting) => out.push(LineEvent::Alerting(key)),
            (S::Dialing | S::Alerting, S::Active) => out.push(LineEvent::Connected(key)),
            _ => {}
        }
    }
}

impl Line for ModemLine {
    fn serves(&self, way: Way) -> bool {
        way == Way::Cellular(self.slot)
    }

    fn dial(&mut self, _way: Way, number: &str, _video: bool) -> Result<Key, LineError> {
        self.call(&ModemDial {
            number: number.to_owned(),
        })?;
        self.next_key += 1;
        self.pending.push(self.next_key);
        Ok(self.next_key)
    }

    fn answer(&mut self, _key: Key) -> Result<(), LineError> {
        self.call(&ModemAnswer)
    }

    fn release(&mut self, key: Key) -> Result<(), LineError> {
        match self.index_of(key) {
            Ok(index) => {
                self.known.remove(&index);
                self.call(&ModemRelease { index })
            }
            // Dialled but not listed yet: forget it; the modem drops what it never set up.
            Err(_) => {
                self.pending.retain(|k| *k != key);
                Ok(())
            }
        }
    }

    fn hold(&mut self, key: Key, on: bool) -> Result<(), LineError> {
        let index = self.index_of(key)?;
        self.call(&ModemHold { index, on })
    }

    fn dtmf(&mut self, _key: Key, tone: u8) -> Result<(), LineError> {
        self.call(&ModemDtmf { key: tone })
    }

    fn media(&mut self, _key: Key, _media: Media) -> Result<(), LineError> {
        Err(LineError::Unsupported)
    }

    fn send_text(&mut self, _way: Way, _number: &str, _text: &str) -> Result<(), LineError> {
        Err(LineError::Unsupported)
    }

    fn cameras(&self) -> u8 {
        0
    }

    fn poll(&mut self) -> Vec<LineEvent> {
        let mut out = Vec::new();
        while let Ok(Some(state)) = self.states.try_recv() {
            self.diff(&state, &mut out);
        }
        out
    }
}

/// Mutes the modem's microphone (the audio route of the mobile voice path).
pub fn modem_mute(bus: &Client, on: bool, timeout: Duration) -> Result<(), LineError> {
    bus.call(&ModemMute { on }, timeout)
        .map(|_| ())
        .map_err(line_err)
}

/// Microphone of the mobile voice path through the modem; the output stays the earpiece
/// (the modem's speaker routing is not part of the modem contract yet).
pub struct ModemAudio {
    bus: Client,
    timeout: Duration,
}

impl ModemAudio {
    /// The route over `bus`.
    pub fn new(bus: Client, timeout: Duration) -> ModemAudio {
        ModemAudio { bus, timeout }
    }
}

impl crate::line::AudioRoute for ModemAudio {
    fn set_mute(&mut self, on: bool) -> Result<(), LineError> {
        modem_mute(&self.bus, on, self.timeout)
    }

    fn set_output(&mut self, output: ultar_bus::messages::call::Output) -> Result<(), LineError> {
        match output {
            ultar_bus::messages::call::Output::Earpiece => Ok(()),
            _ => Err(LineError::Unsupported),
        }
    }

    fn outputs(&self) -> Vec<ultar_bus::messages::call::Output> {
        vec![ultar_bus::messages::call::Output::Earpiece]
    }
}
