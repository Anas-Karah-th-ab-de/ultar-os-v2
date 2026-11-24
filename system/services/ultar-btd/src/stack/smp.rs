//! The Security Manager Protocol (Core Vol 3 Part H), **LE Secure Connections only**:
//! Just Works, Numeric Comparison and Passkey Entry, both roles, as a state machine
//! without I/O. Legacy pairing is refused (`AuthRequirements`), keys shorter than 128
//! bits are refused (`EncryptionKeySize`), a peer public key equal to ours is refused
//! (reflection, CVE-2020-26558), an unexpected PDU ends the pairing. The primitives come
//! from [`super::crypto`]; this file only orders them.

use super::crypto::{f4, f5, f6, g2, random, swap, CryptoError, KeyPair};
use super::hci::Addr;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

#[path = "smp_types.rs"]
mod types;
pub use types::{reason, Ask, Io, Keys, Out};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Method {
    JustWorks,
    Numeric,
    Passkey,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    WaitResponse,
    WaitPublicKey,
    WaitConfirm,
    WaitRandom,
    WaitUser,
    WaitCheck,
    WaitEncryption,
    WaitIdentity,
    WaitIdentityAddr,
    Over,
}

const SC_BONDING: u8 = 0x01 | 0x08; // bonding + Secure Connections
const MITM: u8 = 0x04;
const ID_KEY: u8 = 0x02;

/// One pairing on one LE link.
pub struct Smp {
    central: bool,
    io: Io,
    phase: Phase,
    local: Addr,
    peer: Addr,
    preq: [u8; 7],
    pres: [u8; 7],
    method: Method,
    k: Secrets,
    n: Nonces,
    passkey: Option<u32>,
    round: u8,
    confirmed: bool,
    pending_check: Option<[u8; 16]>,
}

/// The key material of one pairing (zeroed when dropped).
#[derive(Default)]
struct Secrets {
    keys: Option<KeyPair>,
    peer_pk: ([u8; 32], [u8; 32]),
    dh: Zeroizing<[u8; 32]>,
    ltk: Zeroizing<[u8; 16]>,
    irk: Option<Zeroizing<[u8; 16]>>,
}

/// Our nonce, the peer's nonce and the peer's confirm value (big-endian).
#[derive(Default)]
struct Nonces {
    local: [u8; 16],
    peer: [u8; 16],
    c_peer: Option<[u8; 16]>,
}

fn fail(reason: u8) -> Vec<Out> {
    vec![Out::Send(vec![0x05, reason]), Out::Failed(reason)]
}

fn auth_bytes(p: &[u8; 7]) -> [u8; 3] {
    [p[3], p[2], p[1]]
}

impl Smp {
    /// A pairing on a link: `central` = we initiated the link (and so the pairing).
    pub fn new(central: bool, io: Io, local: Addr, peer: Addr) -> Self {
        Self {
            central,
            io,
            phase: Phase::Idle,
            local,
            peer,
            preq: [0; 7],
            pres: [0; 7],
            method: Method::JustWorks,
            k: Secrets::default(),
            n: Nonces::default(),
            passkey: None,
            round: 0,
            confirmed: false,
            pending_check: None,
        }
    }

    /// Still running (not done, not failed, not idle).
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Over)
    }

    /// Central: begins pairing.
    pub fn start(&mut self) -> Vec<Out> {
        if !self.central || self.phase != Phase::Idle {
            return Vec::new();
        }
        self.preq = [0x01, self.io as u8, 0, SC_BONDING | MITM, 16, 0, ID_KEY];
        self.phase = Phase::WaitResponse;
        vec![Out::Send(self.preq.to_vec())]
    }

    /// Ends the pairing locally (timeout, link gone).
    pub fn abort(&mut self, reason: u8) -> Vec<Out> {
        if !self.active() {
            return Vec::new();
        }
        self.phase = Phase::Over;
        if reason == reason::TIMEOUT {
            return vec![Out::Failed(reason)];
        }
        fail(reason)
    }

    /// A PDU from the peer. `pairable`: a request from the peer may start a pairing.
    /// After the end, PDUs are dropped silently (one Pairing Failed was enough).
    pub fn pdu(&mut self, pdu: &[u8], pairable: bool) -> Vec<Out> {
        if self.phase == Phase::Over {
            return Vec::new();
        }
        match self.step(pdu, pairable) {
            Ok(out) => out,
            Err(reason) => {
                self.phase = Phase::Over;
                if pdu.first() == Some(&0x05) {
                    return vec![Out::Failed(reason)];
                }
                fail(reason)
            }
        }
    }

    fn step(&mut self, pdu: &[u8], pairable: bool) -> Result<Vec<Out>, u8> {
        let (&code, body) = pdu.split_first().ok_or(reason::INVALID_PARAMS)?;
        match (code, self.phase) {
            (0x05, _) => Err(body.first().copied().unwrap_or(reason::UNSPECIFIED)),
            (0x01, Phase::Idle) if !self.central => self.on_request(pdu, pairable),
            (0x02, Phase::WaitResponse) => self.on_response(pdu),
            (0x0C, Phase::WaitPublicKey) => self.on_public_key(body),
            (0x03, Phase::WaitConfirm) => self.on_confirm(body),
            (0x04, Phase::WaitRandom) => self.on_random(body),
            (0x0D, Phase::WaitUser) if !self.central => {
                self.pending_check = Some(arr16(body)?);
                Ok(Vec::new())
            }
            (0x0D, Phase::WaitCheck) => self.on_check(body),
            (0x08, Phase::WaitIdentity) => {
                self.k.irk = Some(Zeroizing::new(swap(&arr16(body)?)));
                self.phase = Phase::WaitIdentityAddr;
                Ok(Vec::new())
            }
            (0x09, Phase::WaitIdentityAddr) => self.on_identity_addr(body),
            (0x0B, Phase::Idle) if self.central => Ok(self.start()),
            (0x01, _) if !pairable => Err(reason::NOT_SUPPORTED),
            _ => Err(reason::UNSPECIFIED),
        }
    }

    fn check_features(&self, p: &[u8; 7]) -> Result<(), u8> {
        if p[3] & 0x08 == 0 {
            return Err(reason::AUTH_REQUIREMENTS);
        }
        if p[4] != 16 {
            return Err(reason::KEY_SIZE);
        }
        if p[1] > 4 || p[2] > 1 {
            return Err(reason::INVALID_PARAMS);
        }
        Ok(())
    }

    fn on_request(&mut self, pdu: &[u8], pairable: bool) -> Result<Vec<Out>, u8> {
        if !pairable {
            return Err(reason::NOT_SUPPORTED);
        }
        self.preq = pdu.try_into().map_err(|_| reason::INVALID_PARAMS)?;
        self.check_features(&self.preq)?;
        // We distribute no keys (the SC LTK is derived; no IRK of ours is given out).
        self.pres = [0x02, self.io as u8, 0, SC_BONDING | MITM, 16, 0, 0];
        self.method = self.choose();
        self.k.keys = Some(KeyPair::generate().map_err(crypto)?);
        self.phase = Phase::WaitPublicKey;
        Ok(vec![Out::Send(self.pres.to_vec())])
    }

    fn on_response(&mut self, pdu: &[u8]) -> Result<Vec<Out>, u8> {
        self.pres = pdu.try_into().map_err(|_| reason::INVALID_PARAMS)?;
        self.check_features(&self.pres)?;
        self.method = self.choose();
        let keys = KeyPair::generate().map_err(crypto)?;
        let out = public_key_pdu(&keys);
        self.k.keys = Some(keys);
        self.phase = Phase::WaitPublicKey;
        Ok(vec![Out::Send(out)])
    }

    /// Method of Core Vol 3 Part H 2.3.5.1 (Secure Connections) from both IO capabilities.
    fn choose(&self) -> Method {
        let (a, b) = (self.preq, self.pres);
        if (a[3] | b[3]) & MITM == 0 {
            return Method::JustWorks;
        }
        let (ia, ib) = (a[1], b[1]);
        let (no, disp, yesno, kbd, both) = (3u8, 0u8, 1u8, 2u8, 4u8);
        if ia == no || ib == no {
            return Method::JustWorks;
        }
        let shows = |x: u8| x == yesno || x == both;
        if shows(ia) && shows(ib) {
            return Method::Numeric;
        }
        if ia == kbd || ib == kbd || (ia == both && ib == disp) || (ia == disp && ib == both) {
            return Method::Passkey;
        }
        Method::JustWorks
    }

    fn on_public_key(&mut self, body: &[u8]) -> Result<Vec<Out>, u8> {
        if body.len() != 64 {
            return Err(reason::INVALID_PARAMS);
        }
        let mut x = [0u8; 32];
        let mut y = [0u8; 32];
        x.copy_from_slice(&body[..32]);
        y.copy_from_slice(&body[32..]);
        let (x, y) = (swap(&x), swap(&y));
        let keys = self.k.keys.as_ref().ok_or(reason::UNSPECIFIED)?;
        if bool::from(x.ct_eq(&keys.x)) {
            return Err(reason::INVALID_PARAMS);
        }
        self.k.dh = keys.dh_key(&x, &y).map_err(|_| reason::DHKEY_FAILED)?;
        self.k.peer_pk = (x, y);
        let mut out = Vec::new();
        if !self.central {
            out.push(Out::Send(public_key_pdu(keys)));
        }
        match self.method {
            Method::Passkey => out.extend(self.passkey_begin()?),
            _ if self.central => self.phase = Phase::WaitConfirm,
            _ => {
                self.n.local = random().map_err(crypto)?;
                let c = f4(&self.local_x(), &self.k.peer_pk.0, &self.n.local, 0);
                out.push(Out::Send(pdu16(0x03, &c)));
                self.phase = Phase::WaitRandom;
            }
        }
        Ok(out)
    }

    fn passkey_begin(&mut self) -> Result<Vec<Out>, u8> {
        let we_show = match self.io {
            Io::DisplayOnly | Io::DisplayYesNo => true,
            Io::KeyboardOnly => false,
            _ => {
                let peer_io = if self.central {
                    self.pres[1]
                } else {
                    self.preq[1]
                };
                // Both keyboard+display: the initiator displays (our choice, either works).
                peer_io == Io::KeyboardOnly as u8 || (peer_io == 4 && self.central)
            }
        };
        self.phase = Phase::WaitConfirm;
        if !we_show {
            return Ok(vec![Out::Ask(Ask::Type)]);
        }
        let key = u32::from_le_bytes(random::<4>().map_err(crypto)?) % 1_000_000;
        self.passkey = Some(key);
        let mut out = vec![Out::Ask(Ask::Show(key))];
        if self.central {
            out.extend(self.passkey_confirm()?);
        }
        Ok(out)
    }

    fn r_bit(&self) -> u8 {
        let key = self.passkey.unwrap_or(0);
        0x80 | ((key >> self.round) & 1) as u8
    }

    fn passkey_confirm(&mut self) -> Result<Vec<Out>, u8> {
        self.n.local = random().map_err(crypto)?;
        let c = f4(
            &self.local_x(),
            &self.k.peer_pk.0,
            &self.n.local,
            self.r_bit(),
        );
        Ok(vec![Out::Send(pdu16(0x03, &c))])
    }

    fn on_confirm(&mut self, body: &[u8]) -> Result<Vec<Out>, u8> {
        self.n.c_peer = Some(arr16(body)?);
        self.phase = Phase::WaitRandom;
        if self.method != Method::Passkey {
            self.n.local = random().map_err(crypto)?;
            return Ok(vec![Out::Send(pdu16(0x04, &self.n.local))]);
        }
        if self.central {
            return Ok(vec![Out::Send(pdu16(0x04, &self.n.local))]);
        }
        if self.passkey.is_none() {
            // The responder answers once its person typed the number (see `typed`).
            return Ok(Vec::new());
        }
        self.passkey_confirm()
    }

    fn on_random(&mut self, body: &[u8]) -> Result<Vec<Out>, u8> {
        self.n.peer = arr16(body)?;
        let z = if self.method == Method::Passkey {
            self.r_bit()
        } else {
            0
        };
        if self.central || self.method == Method::Passkey {
            let c = f4(&self.k.peer_pk.0, &self.local_x(), &self.n.peer, z);
            let ok = self
                .n
                .c_peer
                .map(|p| bool::from(p.ct_eq(&c)))
                .unwrap_or(false);
            if !ok {
                return Err(if z == 0 {
                    reason::CONFIRM_FAILED
                } else {
                    reason::PASSKEY_FAILED
                });
            }
        }
        if self.method == Method::Passkey {
            return self.passkey_round_done();
        }
        let mut out = Vec::new();
        if !self.central {
            out.push(Out::Send(pdu16(0x04, &self.n.local)));
        }
        let (na, nb) = self.nonces();
        let (pka, pkb) = self.xs();
        let ask = if self.method == Method::Numeric {
            Ask::Compare(g2(&pka, &pkb, &na, &nb))
        } else {
            Ask::Consent
        };
        out.push(Out::Ask(ask));
        self.phase = Phase::WaitUser;
        Ok(out)
    }

    fn passkey_round_done(&mut self) -> Result<Vec<Out>, u8> {
        let mut out = Vec::new();
        if !self.central {
            out.push(Out::Send(pdu16(0x04, &self.n.local)));
        }
        self.round += 1;
        self.n.c_peer = None;
        if self.round < 20 {
            self.phase = Phase::WaitConfirm;
            if self.central {
                out.extend(self.passkey_confirm()?);
            }
            return Ok(out);
        }
        self.confirmed = true;
        self.phase = Phase::WaitCheck;
        if self.central {
            out.push(Out::Send(pdu16(0x0D, &self.check_value(true))));
        }
        Ok(out)
    }

    /// The person answered [`Ask::Consent`] / [`Ask::Compare`] (or cancelled a
    /// [`Ask::Show`]: `false`).
    pub fn answer(&mut self, accept: bool) -> Vec<Out> {
        if !self.active() {
            return Vec::new();
        }
        if !accept {
            let r = match self.method {
                Method::Numeric => reason::NUMERIC_FAILED,
                Method::Passkey => reason::PASSKEY_FAILED,
                Method::JustWorks => reason::UNSPECIFIED,
            };
            self.phase = Phase::Over;
            return fail(r);
        }
        if self.phase != Phase::WaitUser {
            return Vec::new();
        }
        self.confirmed = true;
        self.phase = Phase::WaitCheck;
        if self.central {
            return vec![Out::Send(pdu16(0x0D, &self.check_value(true)))];
        }
        match self.pending_check.take() {
            Some(e) => self.pdu(&pdu16(0x0D, &e), false),
            None => Vec::new(),
        }
    }

    /// The person typed the passkey shown by the other device ([`Ask::Type`]).
    pub fn typed(&mut self, passkey: u32) -> Vec<Out> {
        if self.method != Method::Passkey || self.passkey.is_some() || !self.active() {
            return Vec::new();
        }
        if passkey >= 1_000_000 {
            return self.answer(false);
        }
        self.passkey = Some(passkey);
        // The central starts the rounds; a peripheral answers a confirm that waited.
        let res = if self.central || self.n.c_peer.is_some() {
            self.passkey_confirm()
        } else {
            Ok(Vec::new())
        };
        res.unwrap_or_else(|r| {
            self.phase = Phase::Over;
            fail(r)
        })
    }

    fn mac_ltk(&self) -> (Zeroizing<[u8; 16]>, Zeroizing<[u8; 16]>) {
        let (na, nb) = self.nonces();
        let (a, b) = self.addrs();
        f5(&self.k.dh, &na, &nb, &a, &b)
    }

    /// `Ea` (initiator's, `ours == central`) or `Eb`.
    fn check_value(&mut self, ours: bool) -> [u8; 16] {
        let (mac, ltk) = self.mac_ltk();
        self.k.ltk = Zeroizing::new(swap(&ltk));
        let (na, nb) = self.nonces();
        let (a, b) = self.addrs();
        let mut r = [0u8; 16];
        if self.method == Method::Passkey {
            r[12..].copy_from_slice(&self.passkey.unwrap_or(0).to_be_bytes());
        }
        let initiator = ours == self.central;
        if initiator {
            f6(&mac, &na, &nb, &r, &auth_bytes(&self.preq), &a, &b)
        } else {
            f6(&mac, &nb, &na, &r, &auth_bytes(&self.pres), &b, &a)
        }
    }

    fn on_check(&mut self, body: &[u8]) -> Result<Vec<Out>, u8> {
        let got = arr16(body)?;
        let expected = self.check_value(false);
        if !bool::from(got.ct_eq(&expected)) || !self.confirmed {
            return Err(reason::DHKEY_FAILED);
        }
        self.phase = Phase::WaitEncryption;
        if self.central {
            return Ok(vec![Out::Encrypt(self.k.ltk.clone())]);
        }
        Ok(vec![Out::Send(pdu16(0x0D, &self.check_value(true)))])
    }

    /// Peripheral: the LTK the controller asks for during this pairing.
    pub fn ltk(&self) -> Option<Zeroizing<[u8; 16]>> {
        (self.phase == Phase::WaitEncryption).then(|| self.k.ltk.clone())
    }

    /// The link is encrypted (or failed to: `ok == false`).
    pub fn encrypted(&mut self, ok: bool) -> Vec<Out> {
        if self.phase != Phase::WaitEncryption {
            return Vec::new();
        }
        if !ok {
            self.phase = Phase::Over;
            return vec![Out::Failed(reason::UNSPECIFIED)];
        }
        if self.central && self.pres[6] & ID_KEY != 0 {
            self.phase = Phase::WaitIdentity;
            return Vec::new();
        }
        self.finish(None)
    }

    fn on_identity_addr(&mut self, body: &[u8]) -> Result<Vec<Out>, u8> {
        if body.len() != 7 || body[0] > 1 {
            return Err(reason::INVALID_PARAMS);
        }
        let mut bytes = [0u8; 6];
        bytes.copy_from_slice(&body[1..]);
        let addr = Addr {
            bytes,
            random: body[0] == 1,
        };
        let irk = self.k.irk.take().ok_or(reason::UNSPECIFIED)?;
        Ok(self.finish(Some((irk, addr))))
    }

    fn finish(&mut self, identity: Option<(Zeroizing<[u8; 16]>, Addr)>) -> Vec<Out> {
        self.phase = Phase::Over;
        vec![Out::Done(Keys {
            ltk: self.k.ltk.clone(),
            authenticated: self.method != Method::JustWorks,
            identity,
        })]
    }

    fn local_x(&self) -> [u8; 32] {
        self.k.keys.as_ref().map(|k| k.x).unwrap_or([0; 32])
    }

    fn xs(&self) -> ([u8; 32], [u8; 32]) {
        if self.central {
            (self.local_x(), self.k.peer_pk.0)
        } else {
            (self.k.peer_pk.0, self.local_x())
        }
    }

    fn nonces(&self) -> ([u8; 16], [u8; 16]) {
        if self.central {
            (self.n.local, self.n.peer)
        } else {
            (self.n.peer, self.n.local)
        }
    }

    fn addrs(&self) -> ([u8; 7], [u8; 7]) {
        if self.central {
            (self.local.smp_value(), self.peer.smp_value())
        } else {
            (self.peer.smp_value(), self.local.smp_value())
        }
    }
}

fn crypto(_: CryptoError) -> u8 {
    reason::UNSPECIFIED
}

fn arr16(body: &[u8]) -> Result<[u8; 16], u8> {
    let a: [u8; 16] = body.try_into().map_err(|_| reason::INVALID_PARAMS)?;
    Ok(swap(&a))
}

fn pdu16(code: u8, v: &[u8; 16]) -> Vec<u8> {
    let mut out = vec![code];
    out.extend_from_slice(&swap(v));
    out
}

fn public_key_pdu(k: &KeyPair) -> Vec<u8> {
    let mut out = vec![0x0C];
    out.extend_from_slice(&swap(&k.x));
    out.extend_from_slice(&swap(&k.y));
    out
}

#[cfg(test)]
#[path = "smp_tests.rs"]
mod tests;
