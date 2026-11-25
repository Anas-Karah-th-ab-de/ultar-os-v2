//! The call's handshake and signalling as a state machine without network or threads: what to
//! send, what to tell the phone, when the media start (see [`crate::e2e::crypto`] for the keys).
//!
//! On the signalling server every message is `{"to": <number>, "type": "e2e", "data": …}`: the
//! server learns which number talks to which, when and how much – never what. Only `hello` and
//! `reply` are readable (ephemeral keys and the public device keys); everything after them –
//! video or not, addresses, ringing, accepting, hanging up – is sealed.

use crate::e2e::crypto::{self, CallKeys, Ephemeral, MediaKeys, Role, Transcript, CALL_ID};
use crate::e2e::pins::{Pin, Pins};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde_json::{json, Value};
use std::net::SocketAddr;

/// The device key of this device, kept by the account service.
pub trait Signer: Send {
    /// Its public key.
    fn public(&self) -> Option<[u8; 32]>;
    /// Signature over `message` (only ever `crypto::SIGN_LABEL ‖ T`).
    fn sign(&self, message: &[u8]) -> Option<[u8; 64]>;
}

/// What the media part needs to start.
pub struct Plan {
    /// The media keys.
    pub media: MediaKeys,
    /// This device's role.
    pub role: Role,
    /// The other side's addresses.
    pub peer: Vec<SocketAddr>,
    /// Video call.
    pub video: bool,
}

/// What the engine asks for.
pub enum Out {
    /// Send this to `to` over the signalling server.
    Send {
        /// Number.
        to: String,
        /// The message (`data`).
        data: String,
    },
    /// A call comes in (after the caller proved its device key).
    Incoming {
        /// Number.
        from: String,
        /// Video offered.
        video: bool,
    },
    /// The other side rings.
    Ringing,
    /// Both sides agreed; the safety number of the two devices; whether the other device's key
    /// differs from the one seen before for this number.
    Secured {
        /// 12 groups of 5 digits.
        safety: String,
        /// The number's device key changed.
        changed: bool,
    },
    /// Start the media.
    Start(Box<Plan>),
    /// The call is over (`reason`: `remote`, `failed`, `unreachable`).
    Ended(&'static str),
}

struct Mine {
    call: [u8; CALL_ID],
    peer: String,
    video: bool,
    cands: Vec<SocketAddr>,
}

enum Stage {
    Idle,
    /// Hello sent, waiting for the reply.
    Calling(Mine, Ephemeral),
    /// Reply checked, confirmation sent; waiting for ring/accept.
    Waiting(Mine, Box<CallKeys>),
    /// Reply sent, waiting for the caller's confirmation.
    Replied(Mine, Box<CallKeys>, Transcript),
    /// Ringing here.
    Ringing(Mine, Box<CallKeys>, Vec<SocketAddr>),
    /// Media running.
    Active(Mine, Box<CallKeys>),
}

/// The engine of one device (one call at a time).
pub struct Engine {
    signer: Box<dyn Signer>,
    pins: Pins,
    stage: Stage,
}

fn b64(b: &[u8]) -> String {
    B64.encode(b)
}

fn arr<const N: usize>(v: &Value, k: &str) -> Option<[u8; N]> {
    B64.decode(v.get(k)?.as_str()?).ok()?.try_into().ok()
}

fn addrs(v: &Value) -> Vec<SocketAddr> {
    v.get("cand")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str()?.parse().ok())
                .take(8)
                .collect()
        })
        .unwrap_or_default()
}

fn cand_json(c: &[SocketAddr]) -> Value {
    Value::from(c.iter().map(ToString::to_string).collect::<Vec<_>>())
}

impl Engine {
    /// An engine over this device key and the pinned keys of other numbers.
    pub fn new(signer: Box<dyn Signer>, pins: Pins) -> Self {
        Self {
            signer,
            pins,
            stage: Stage::Idle,
        }
    }

    /// True while a call exists.
    pub fn busy(&self) -> bool {
        !matches!(self.stage, Stage::Idle)
    }

    fn send(to: &str, msg: &Value) -> Out {
        Out::Send {
            to: to.to_owned(),
            data: msg.to_string(),
        }
    }

    fn sealed(keys: &mut CallKeys, call: &[u8; CALL_ID], body: &Value) -> Value {
        let c = keys.signal_out.seal(call, body.to_string().as_bytes());
        json!({"k": "s", "call": hex::encode(call), "c": b64(&c)})
    }

    /// Places a call; `cands` are this device's addresses.
    pub fn dial(&mut self, peer: &str, video: bool, cands: Vec<SocketAddr>) -> Vec<Out> {
        let Some(da) = self.signer.public() else {
            return vec![Out::Ended("failed")];
        };
        if self.busy() {
            return Vec::new();
        }
        let mut call = [0u8; CALL_ID];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut call);
        let eph = Ephemeral::new();
        let hello = json!({"k": "hello", "call": hex::encode(call), "ea": b64(&eph.public), "da": b64(&da)});
        let out = vec![Self::send(peer, &hello)];
        let mine = Mine {
            call,
            peer: peer.to_owned(),
            video,
            cands,
        };
        self.stage = Stage::Calling(mine, eph);
        out
    }

    /// Accepts the ringing call; `cands` are this device's addresses.
    pub fn answer(&mut self, cands: Vec<SocketAddr>) -> Vec<Out> {
        let Stage::Ringing(mut mine, mut keys, peer_cands) =
            std::mem::replace(&mut self.stage, Stage::Idle)
        else {
            return Vec::new();
        };
        mine.cands = cands;
        let msg = Self::sealed(
            &mut keys,
            &mine.call,
            &json!({"t": "accept", "cand": cand_json(&mine.cands)}),
        );
        let out = vec![
            Self::send(&mine.peer, &msg),
            Self::start(&mut keys, Role::Callee, peer_cands, mine.video),
        ];
        self.stage = Stage::Active(mine, keys);
        out
    }

    fn start(keys: &mut CallKeys, role: Role, peer: Vec<SocketAddr>, video: bool) -> Out {
        match keys.media.take() {
            Some(media) => Out::Start(Box::new(Plan {
                media,
                role,
                peer,
                video,
            })),
            None => Out::Ended("failed"),
        }
    }

    /// Ends or declines the call.
    pub fn hangup(&mut self) -> Vec<Out> {
        match std::mem::replace(&mut self.stage, Stage::Idle) {
            Stage::Idle => Vec::new(),
            Stage::Calling(m, _) => vec![
                Self::send(&m.peer, &json!({"k": "bye", "call": hex::encode(m.call)})),
                Out::Ended("local"),
            ],
            Stage::Waiting(m, mut k)
            | Stage::Replied(m, mut k, _)
            | Stage::Ringing(m, mut k, _)
            | Stage::Active(m, mut k) => {
                let bye = Self::sealed(&mut k, &m.call, &json!({"t": "bye"}));
                vec![Self::send(&m.peer, &bye), Out::Ended("local")]
            }
        }
    }

    /// A message of the signalling server (`from` as the server wrote it).
    pub fn on_signal(
        &mut self,
        from: &str,
        v: &Value,
        cands: impl FnOnce() -> Vec<SocketAddr>,
    ) -> Vec<Out> {
        if v.get("type").and_then(Value::as_str) == Some("unreachable") {
            return self.lost_peer(v.get("to").and_then(Value::as_str).unwrap_or_default());
        }
        let Some(data) = v
            .get("data")
            .and_then(Value::as_str)
            .and_then(|d| serde_json::from_str::<Value>(d).ok())
        else {
            return Vec::new();
        };
        match data.get("k").and_then(Value::as_str) {
            Some("hello") => self.on_hello(from, &data, cands),
            Some("reply") => self.on_reply(from, &data),
            Some("s") => self.on_sealed(from, &data),
            Some("bye") => self.on_bye_plain(from, &data),
            _ => Vec::new(),
        }
    }

    fn lost_peer(&mut self, to: &str) -> Vec<Out> {
        let ours = match &self.stage {
            Stage::Calling(m, _) | Stage::Waiting(m, _) => m.peer == to,
            _ => false,
        };
        if ours {
            self.stage = Stage::Idle;
            return vec![Out::Ended("unreachable")];
        }
        Vec::new()
    }

    fn on_hello(
        &mut self,
        from: &str,
        d: &Value,
        cands: impl FnOnce() -> Vec<SocketAddr>,
    ) -> Vec<Out> {
        let (Some(call), Some(ea), Some(da), Some(db)) = (
            d.get("call")
                .and_then(Value::as_str)
                .and_then(|c| hex::decode(c).ok()?.try_into().ok()),
            arr::<32>(d, "ea"),
            arr::<32>(d, "da"),
            self.signer.public(),
        ) else {
            return Vec::new();
        };
        if self.busy() {
            return Vec::new(); // the caller hears nothing back and gives up
        }
        let eb = Ephemeral::new();
        let t = Transcript {
            call,
            ea,
            eb: eb.public,
            da,
            db,
        };
        let Some(sig) = self.signer.sign(&t.to_sign()) else {
            return Vec::new();
        };
        let Ok(keys) = crypto::keys(eb, &ea, &t, Role::Callee) else {
            return Vec::new();
        };
        let reply = json!({"k": "reply", "call": hex::encode(call), "eb": b64(&t.eb), "db": b64(&db), "sig": b64(&sig)});
        let mine = Mine {
            call,
            peer: from.to_owned(),
            video: false,
            cands: cands(),
        };
        self.stage = Stage::Replied(mine, Box::new(keys), t);
        vec![Self::send(from, &reply)]
    }

    fn on_reply(&mut self, from: &str, d: &Value) -> Vec<Out> {
        let Stage::Calling(mine, eph) = std::mem::replace(&mut self.stage, Stage::Idle) else {
            return Vec::new();
        };
        let (Some(eb), Some(db), Some(sig), Some(da)) = (
            arr::<32>(d, "eb"),
            arr::<32>(d, "db"),
            d.get("sig")
                .and_then(Value::as_str)
                .and_then(|s| B64.decode(s).ok()),
            self.signer.public(),
        ) else {
            self.stage = Stage::Calling(mine, eph);
            return Vec::new();
        };
        if from != mine.peer {
            self.stage = Stage::Calling(mine, eph);
            return Vec::new();
        }
        let t = Transcript {
            call: mine.call,
            ea: eph.public,
            eb,
            da,
            db,
        };
        if crypto::verify(&t, &db, &sig).is_err() {
            return vec![Out::Ended("failed")];
        }
        let (Some(own_sig), Ok(mut keys)) = (
            self.signer.sign(&t.to_sign()),
            crypto::keys(eph, &eb, &t, Role::Caller),
        ) else {
            return vec![Out::Ended("failed")];
        };
        let confirm = json!({"t": "confirm", "sig": b64(&own_sig), "video": mine.video, "cand": cand_json(&mine.cands)});
        let msg = Self::sealed(&mut keys, &mine.call, &confirm);
        let mut out = vec![Self::send(&mine.peer, &msg)];
        out.push(self.secured(&mine.peer, &da, &db));
        self.stage = Stage::Waiting(mine, Box::new(keys));
        out
    }

    fn secured(&mut self, number: &str, own: &[u8; 32], other: &[u8; 32]) -> Out {
        let changed = matches!(self.pins.check(number, other), Pin::Changed);
        Out::Secured {
            safety: crypto::safety_number(own, other),
            changed,
        }
    }

    fn on_bye_plain(&mut self, from: &str, d: &Value) -> Vec<Out> {
        // Before keys exist the caller can only say bye in the clear (it reveals nothing).
        let same = |m: &Mine| {
            m.peer == from
                && d.get("call").and_then(Value::as_str) == Some(hex::encode(m.call).as_str())
        };
        if matches!(&self.stage, Stage::Replied(m, _, _) if same(m)) {
            self.stage = Stage::Idle;
        }
        Vec::new()
    }

    fn on_sealed(&mut self, from: &str, d: &Value) -> Vec<Out> {
        let stage = std::mem::replace(&mut self.stage, Stage::Idle);
        let (mine, mut keys) = match stage {
            Stage::Waiting(m, k) | Stage::Ringing(m, k, _) | Stage::Active(m, k) => (m, k),
            Stage::Replied(m, k, t) => return self.on_confirm(from, d, m, k, t),
            other => {
                self.stage = other;
                return Vec::new();
            }
        };
        let body = Self::open(from, d, &mine, &mut keys);
        match body
            .as_ref()
            .and_then(|b| b.get("t"))
            .and_then(Value::as_str)
        {
            Some("bye") => vec![Out::Ended("remote")],
            Some("ring") => {
                self.stage = Stage::Waiting(mine, keys);
                vec![Out::Ringing]
            }
            Some("accept") => {
                let peer = body.as_ref().map(addrs).unwrap_or_default();
                let video = mine.video;
                let start = Self::start(&mut keys, Role::Caller, peer, video);
                self.stage = Stage::Active(mine, keys);
                vec![start]
            }
            _ => {
                // not ours or not readable: keep the call as it was
                self.stage = Stage::Waiting(mine, keys);
                Vec::new()
            }
        }
    }

    fn open(from: &str, d: &Value, mine: &Mine, keys: &mut CallKeys) -> Option<Value> {
        let call_ok =
            d.get("call").and_then(Value::as_str) == Some(hex::encode(mine.call).as_str());
        if from != mine.peer || !call_ok {
            return None;
        }
        let c = B64.decode(d.get("c")?.as_str()?).ok()?;
        let plain = keys.signal_in.open(&mine.call, &c).ok()?;
        serde_json::from_slice(&plain).ok()
    }

    fn on_confirm(
        &mut self,
        from: &str,
        d: &Value,
        mut mine: Mine,
        mut keys: Box<CallKeys>,
        t: Transcript,
    ) -> Vec<Out> {
        let body = Self::open(from, d, &mine, &mut keys);
        let sig = body
            .as_ref()
            .and_then(|b| b.get("sig")?.as_str())
            .and_then(|s| B64.decode(s).ok());
        let valid = body
            .as_ref()
            .is_some_and(|b| b.get("t").and_then(Value::as_str) == Some("confirm"))
            && sig.is_some_and(|s| crypto::verify(&t, &t.da, &s).is_ok());
        if !valid {
            return Vec::new(); // a forged or broken confirmation: no ringing, the call fades
        }
        mine.video = body
            .as_ref()
            .and_then(|b| b.get("video")?.as_bool())
            .unwrap_or(false);
        let peer_cands = body.as_ref().map(addrs).unwrap_or_default();
        let ring = Self::sealed(&mut keys, &mine.call, &json!({"t": "ring"}));
        let out = vec![
            Self::send(&mine.peer, &ring),
            Out::Incoming {
                from: mine.peer.clone(),
                video: mine.video,
            },
            self.secured(&mine.peer, &t.db, &t.da),
        ];
        self.stage = Stage::Ringing(mine, keys, peer_cands);
        out
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
