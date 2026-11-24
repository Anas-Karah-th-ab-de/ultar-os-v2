//! The modem on the bus (`modem.*`, contract `ultar_bus::messages::modem`): the daemon calls
//! [`ModemService::serve`] after every step of the engine; it answers the waiting commands of
//! the call service and publishes `modem.state` when it changed.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::engine::{Engine, State};
use crate::link::ModemLink;
use crate::tel::CallStatus;
use ultar_bus::messages::call::{check_number, Cellular};
use ultar_bus::messages::modem::{
    ModemAnswer, ModemCall, ModemCallStatus, ModemDial, ModemDone, ModemDtmf, ModemHold, ModemMute,
    ModemRelease, ModemState, ERR_OFFLINE, ERR_REFUSED, MAX_MODEM_CALLS, SERVICE,
};
use ultar_bus::{BusError, Client, ErrorCode, IncomingRequest, ServiceEndpoint};

/// Raw SIM status "no card" of the modem.
const SIM_ABSENT: u8 = 0x80;
/// Registration "home network" and "roaming".
const REG_HOME: u8 = 2;
/// See [`REG_HOME`].
const REG_ROAMING: u8 = 6;
/// Registration "emergency calls only".
const REG_EMERGENCY: u8 = 4;

/// The contract's view of the engine state of slot `slot`.
pub fn state_of(st: &State, airplane: bool, slot: u8) -> ModemState {
    let sim = st.sim.filter(|s| *s != SIM_ABSENT && *s != 0x81);
    let cellular = match (airplane, sim, st.reg_cs.status) {
        (true, _, _) => Cellular::Airplane,
        (false, None, _) => Cellular::NoSim,
        (false, Some(_), REG_HOME | REG_ROAMING) => Cellular::Ready,
        (false, Some(_), REG_EMERGENCY) => Cellular::EmergencyOnly,
        (false, Some(_), _) => Cellular::NoNetwork,
    };
    let calls = st
        .calls
        .iter()
        .take(MAX_MODEM_CALLS)
        .map(|c| ModemCall {
            index: c.id,
            incoming: c.term == 1,
            number: c.number.clone(),
            status: match c.status {
                CallStatus::Dialing => ModemCallStatus::Dialing,
                CallStatus::Alerting => ModemCallStatus::Alerting,
                CallStatus::Incoming => ModemCallStatus::Incoming,
                CallStatus::Waiting => ModemCallStatus::Waiting,
                CallStatus::Holding => ModemCallStatus::Held,
                CallStatus::Active | CallStatus::Other(_) => ModemCallStatus::Active,
            },
        })
        .collect();
    ModemState {
        cellular,
        sims: sim.map(|_| vec![slot]).unwrap_or_default(),
        calls,
    }
}

/// The modem service on the bus.
pub struct ModemService {
    bus: Client,
    endpoint: ServiceEndpoint,
    published: Option<ModemState>,
}

fn refused(_: std::io::Error) -> ErrorCode {
    ErrorCode::Service(ERR_REFUSED)
}

impl ModemService {
    /// Registers `modem` on `bus`.
    pub fn new(bus: Client) -> Result<ModemService, BusError> {
        let endpoint = bus.register(SERVICE)?;
        Ok(ModemService {
            bus,
            endpoint,
            published: None,
        })
    }

    /// Answers every waiting command and publishes the state if it changed.
    pub fn serve<L: ModemLink>(
        &mut self,
        engine: &mut Engine<L>,
        state: ModemState,
    ) -> Result<(), BusError> {
        while let Some(request) = self.endpoint.try_recv()? {
            let online = state.cellular != Cellular::Airplane && engine.st.powered;
            // A failed answer concerns one request only (caller gone, hostile payload).
            let _ = handle(engine, request, online);
        }
        if self.published.as_ref() != Some(&state) {
            self.bus.publish(&state)?;
            self.published = Some(state);
        }
        Ok(())
    }
}

fn handle<L: ModemLink>(
    engine: &mut Engine<L>,
    request: IncomingRequest,
    online: bool,
) -> Result<(), BusError> {
    if !online {
        return request.fail(ErrorCode::Service(ERR_OFFLINE));
    }
    let done = |r: std::io::Result<()>| r.map(|()| ModemDone).map_err(refused);
    match request.method() {
        "dial" => request.respond::<ModemDial>(|d| {
            check_number(&d.number).map_err(|_| ErrorCode::BadPayload)?;
            done(engine.dial(&d.number))
        }),
        "answer" => request.respond::<ModemAnswer>(|_| done(engine.answer())),
        "release" => request.respond::<ModemRelease>(|r| done(engine.release(r.index))),
        "hold" => request.respond::<ModemHold>(|h| done(engine.hold_swap(h.index))),
        "dtmf" => request.respond::<ModemDtmf>(|d| done(engine.dtmf(d.key))),
        "mute" => request.respond::<ModemMute>(|m| done(engine.mute(m.on))),
        _ => request.fail(ErrorCode::UnknownMethod),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tel::CallEntry;

    #[test]
    fn engine_state_maps_to_the_contract() {
        let mut st = State {
            sim: Some(0x82),
            ..State::default()
        };
        st.reg_cs.status = REG_HOME;
        st.calls = vec![CallEntry {
            id: 2,
            term: 1,
            status: CallStatus::Holding,
            number: "0933".into(),
        }];
        let m = state_of(&st, false, 1);
        assert_eq!(m.cellular, Cellular::Ready);
        assert_eq!(m.sims, vec![1]);
        assert_eq!(m.calls[0].status, ModemCallStatus::Held);
        assert!(m.calls[0].incoming);
        assert_eq!(state_of(&st, true, 1).cellular, Cellular::Airplane);
        st.reg_cs.status = REG_EMERGENCY;
        assert_eq!(state_of(&st, false, 1).cellular, Cellular::EmergencyOnly);
        st.reg_cs.status = 3;
        assert_eq!(state_of(&st, false, 1).cellular, Cellular::NoNetwork);
        st.sim = Some(SIM_ABSENT);
        assert_eq!(state_of(&st, false, 1).cellular, Cellular::NoSim);
    }

    #[test]
    fn builders_and_engine_commands_reach_the_link() {
        use crate::link::MockLink;
        use crate::rfs::Rfs;
        let dir = std::env::temp_dir().join(format!("ril-bus-{}", std::process::id()));
        let mut e = Engine::new(
            MockLink::new(),
            Rfs::new(dir.to_string_lossy().into_owned()),
        );
        e.dtmf(b'5').unwrap();
        assert!(e.dtmf(b'x').is_err());
        e.mute(true).unwrap();
        e.hold_swap(1).unwrap();
        assert!(e.release(3).is_err(), "unknown call index");
        assert!(e.link.sent.len() >= 5);
        assert_eq!(crate::tel::build_manage_call(2, 1), vec![2, 1]);
        assert_eq!(crate::tel::build_mute(false), vec![0]);
        assert_eq!(crate::tel::build_dtmf(1, b'#'), Some(vec![1, b'#']));
    }
}
