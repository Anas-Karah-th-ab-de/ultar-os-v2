//! Test double of the modem service (`modem.*` on the bus) for tests and for the end-to-end
//! run on the Ultar kernel (`ultar-calld --fake-modem`): a call list that dial / answer /
//! release / hold change, driven further by a [`FakeModemControl`] (ring, connect, end).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use ultar_bus::messages::call::Cellular;
use ultar_bus::messages::modem::{
    ModemCall, ModemCallStatus, ModemDial, ModemDone, ModemDtmf, ModemHold, ModemMute,
    ModemRelease, ModemState, MAX_MODEM_CALLS, SERVICE,
};
use ultar_bus::{BusError, Client, ErrorCode, IncomingRequest, ServiceEndpoint};

#[derive(Default)]
struct Inner {
    state: ModemState,
    tones: Vec<u8>,
    muted: bool,
}

/// Drives a [`FakeModem`].
#[derive(Clone, Default)]
pub struct FakeModemControl {
    inner: Arc<Mutex<Inner>>,
}

impl FakeModemControl {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn free_index(calls: &[ModemCall]) -> Option<u8> {
        (1..=MAX_MODEM_CALLS as u8).find(|i| !calls.iter().any(|c| c.index == *i))
    }

    /// Network state and SIM slots.
    pub fn set_cellular(&self, cellular: Cellular, sims: &[u8]) {
        let mut inner = self.lock();
        inner.state.cellular = cellular;
        inner.state.sims = sims.to_vec();
    }

    /// A call comes in; returns its index.
    pub fn ring(&self, number: &str) -> Option<u8> {
        let mut inner = self.lock();
        let index = Self::free_index(&inner.state.calls)?;
        let busy = !inner.state.calls.is_empty();
        let status = if busy {
            ModemCallStatus::Waiting
        } else {
            ModemCallStatus::Incoming
        };
        inner.state.calls.push(ModemCall {
            index,
            incoming: true,
            number: number.into(),
            status,
        });
        Some(index)
    }

    /// Sets the status of call `index`.
    pub fn set_status(&self, index: u8, status: ModemCallStatus) {
        if let Some(c) = self
            .lock()
            .state
            .calls
            .iter_mut()
            .find(|c| c.index == index)
        {
            c.status = status;
        }
    }

    /// The call ends from the other side.
    pub fn end(&self, index: u8) {
        self.lock().state.calls.retain(|c| c.index != index);
    }

    /// The current state.
    pub fn state(&self) -> ModemState {
        self.lock().state.clone()
    }

    /// Tones sent so far and the microphone state.
    pub fn tones_and_mute(&self) -> (Vec<u8>, bool) {
        let inner = self.lock();
        (inner.tones.clone(), inner.muted)
    }
}

/// The fake modem service on the bus.
pub struct FakeModem {
    bus: Client,
    endpoint: ServiceEndpoint,
    control: FakeModemControl,
    published: Option<ModemState>,
}

impl FakeModem {
    /// Registers `modem` on `bus`.
    pub fn new(bus: Client, control: FakeModemControl) -> Result<FakeModem, BusError> {
        let endpoint = bus.register(SERVICE)?;
        Ok(FakeModem {
            bus,
            endpoint,
            control,
            published: None,
        })
    }

    /// Waits up to `tick` for a command, answers it, publishes the state when it changed.
    pub fn step(&mut self, tick: Duration) -> Result<(), BusError> {
        match self.endpoint.recv_timeout(tick) {
            Ok(request) => {
                // A failed answer concerns one request only.
                let _ = self.handle(request);
            }
            Err(BusError::Timeout) => {}
            Err(e) => return Err(e),
        }
        let state = self.control.state();
        if self.published.as_ref() != Some(&state) {
            self.bus.publish(&state)?;
            self.published = Some(state);
        }
        Ok(())
    }

    fn handle(&mut self, request: IncomingRequest) -> Result<(), BusError> {
        let c = self.control.clone();
        match request.method() {
            "dial" => request.respond::<ModemDial>(|d| dial(&c, d)),
            "answer" => request.respond::<ultar_bus::messages::modem::ModemAnswer>(|_| {
                answer(&c);
                Ok(ModemDone)
            }),
            "release" => request.respond::<ModemRelease>(|r| {
                c.end(r.index);
                Ok(ModemDone)
            }),
            "hold" => request.respond::<ModemHold>(|h| {
                let status = if h.on {
                    ModemCallStatus::Held
                } else {
                    ModemCallStatus::Active
                };
                c.set_status(h.index, status);
                Ok(ModemDone)
            }),
            "dtmf" => request.respond::<ModemDtmf>(|d| {
                c.lock().tones.push(d.key);
                Ok(ModemDone)
            }),
            "mute" => request.respond::<ModemMute>(|m| {
                c.lock().muted = m.on;
                Ok(ModemDone)
            }),
            _ => request.fail(ErrorCode::UnknownMethod),
        }
    }
}

/// `modem.dial`: active calls go on hold, the new call alerts at once.
fn dial(c: &FakeModemControl, d: ModemDial) -> Result<ModemDone, ErrorCode> {
    let mut inner = c.lock();
    let index = FakeModemControl::free_index(&inner.state.calls).ok_or(ErrorCode::Service(2))?;
    for call in inner.state.calls.iter_mut() {
        if call.status == ModemCallStatus::Active {
            call.status = ModemCallStatus::Held;
        }
    }
    let status = ModemCallStatus::Alerting;
    inner.state.calls.push(ModemCall {
        index,
        incoming: false,
        number: d.number,
        status,
    });
    Ok(ModemDone)
}

/// `modem.answer`: the ringing call becomes active, an active one goes on hold.
fn answer(c: &FakeModemControl) {
    for call in c.lock().state.calls.iter_mut() {
        call.status = match call.status {
            ModemCallStatus::Incoming | ModemCallStatus::Waiting => ModemCallStatus::Active,
            ModemCallStatus::Active => ModemCallStatus::Held,
            s => s,
        };
    }
}
