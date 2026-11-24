//! The supplicant's side of the 4-way handshake (12.7.6) and the group-key
//! handshake (12.7.7) for WPA2-Personal with CCMP.
//!
//! Protections: the replay counter must grow on every MIC-checked frame;
//! message 3 must carry the ANonce of message 1 and the network's own RSN
//! element (downgrade); a key already installed is never installed again
//! (key reinstallation, "KRACK"): a repeated message 3 or group message is
//! answered, its keys are not handed out a second time.

use super::eapol::{info, KeyFrame};
use super::keys::{Pmk, Ptk};
use super::WpaError;
use crate::ie;
use crate::Mac;
use aes_kw::KekAes128;
use zeroize::{Zeroize, Zeroizing};

/// Length of the CCMP group key.
pub const GTK_LEN: usize = 16;

/// A group key to install.
#[derive(Clone, PartialEq, Eq)]
pub struct GroupKey {
    /// Key index (1–3).
    pub index: u8,
    /// The key.
    pub key: Zeroizing<Vec<u8>>,
    /// Receive sequence counter to start from.
    pub rsc: [u8; 8],
}

impl std::fmt::Debug for GroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GroupKey {{ index: {}, .. }}", self.index)
    }
}

/// What the caller does with a frame's outcome.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Send this frame to the authenticator.
    Send(Vec<u8>),
    /// Send message 4, then install the pairwise key and the group key and
    /// open the port.
    Complete {
        /// Message 4.
        reply: Vec<u8>,
        /// Temporal key (CCMP) for the pairwise key.
        tk: Zeroizing<Vec<u8>>,
        /// The group key.
        group: GroupKey,
    },
    /// Send the group message 2, then install the new group key.
    Rekey {
        /// Group message 2.
        reply: Vec<u8>,
        /// The new group key.
        group: GroupKey,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    WaitMsg1,
    WaitMsg3,
    Done,
}

/// One handshake with one access point.
pub struct Supplicant {
    pmk: Pmk,
    aa: Mac,
    spa: Mac,
    ap_rsn: Vec<u8>,
    own_rsn: Vec<u8>,
    snonce: [u8; 32],
    anonce: Option<[u8; 32]>,
    ptk: Option<Ptk>,
    replay: Option<u64>,
    gtk: Zeroizing<Vec<u8>>,
    phase: Phase,
}

impl Drop for Supplicant {
    fn drop(&mut self) {
        self.snonce.zeroize();
    }
}

impl std::fmt::Debug for Supplicant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Supplicant {{ phase: {:?}, .. }}", self.phase)
    }
}

impl Supplicant {
    /// A handshake of `spa` (own address) with `aa`, whose RSN element (from
    /// its beacon or probe response) is `ap_rsn`; `own_rsn` is the element
    /// sent in the association request, `snonce` 32 fresh random bytes.
    pub fn new(
        pmk: Pmk,
        aa: Mac,
        spa: Mac,
        ap_rsn: Vec<u8>,
        own_rsn: Vec<u8>,
        snonce: [u8; 32],
    ) -> Self {
        Supplicant {
            pmk,
            aa,
            spa,
            ap_rsn,
            own_rsn,
            snonce,
            anonce: None,
            ptk: None,
            replay: None,
            gtk: Zeroizing::new(Vec::new()),
            phase: Phase::WaitMsg1,
        }
    }

    /// The 4-way handshake finished.
    pub fn complete(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Handles one EAPOL frame from the authenticator. An error means: drop
    /// the frame (a wrong MIC on message 3 hints at a wrong passphrase).
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Step, WpaError> {
        let frame = KeyFrame::parse(bytes)?;
        if frame.info & info::VERSION != info::VERSION_2 || !frame.has(info::ACK) {
            return Err(WpaError::Unexpected);
        }
        let pairwise = frame.has(info::PAIRWISE);
        match (pairwise, frame.has(info::MIC)) {
            (true, false) if self.phase != Phase::Done => self.message_1(&frame),
            (true, true) if self.phase != Phase::WaitMsg1 => self.message_3(&frame),
            (false, true) if self.phase == Phase::Done => self.group_1(&frame),
            _ => Err(WpaError::Unexpected),
        }
    }

    fn message_1(&mut self, f: &KeyFrame) -> Result<Step, WpaError> {
        if f.has(info::INSTALL) || f.has(info::ENCRYPTED) {
            return Err(WpaError::Unexpected);
        }
        self.ptk = Some(Ptk::derive(
            &self.pmk,
            &self.aa,
            &self.spa,
            &f.nonce,
            &self.snonce,
        ));
        self.anonce = Some(f.nonce);
        self.phase = Phase::WaitMsg3;
        let reply = KeyFrame {
            version: f.version,
            info: info::MIC | info::PAIRWISE | info::VERSION_2,
            key_len: 0,
            replay: f.replay,
            nonce: self.snonce,
            rsc: [0; 8],
            mic: [0; 16],
            data: self.own_rsn.clone(),
        };
        Ok(Step::Send(self.sign(reply)?))
    }

    fn sign(&self, frame: KeyFrame) -> Result<Vec<u8>, WpaError> {
        let ptk = self.ptk.as_ref().ok_or(WpaError::Unexpected)?;
        Ok(frame.sign(ptk.kck()))
    }

    /// MIC and replay counter of a frame that carries a MIC.
    fn check(&self, f: &KeyFrame) -> Result<&Ptk, WpaError> {
        let ptk = self.ptk.as_ref().ok_or(WpaError::Unexpected)?;
        if self.replay.is_some_and(|r| f.replay <= r) {
            return Err(WpaError::Replay);
        }
        if !f.mic_ok(ptk.kck()) {
            return Err(WpaError::BadMic);
        }
        Ok(ptk)
    }

    fn message_3(&mut self, f: &KeyFrame) -> Result<Step, WpaError> {
        let wanted = info::INSTALL | info::ENCRYPTED | info::SECURE;
        if !f.has(wanted) {
            return Err(WpaError::Unexpected);
        }
        if Some(f.nonce) != self.anonce {
            return Err(WpaError::NonceMismatch);
        }
        let ptk = self.check(f)?;
        let plain = unwrap(ptk.kek(), &f.data)?;
        let ap_rsn = ie::find_raw(&plain, ie::RSN).ok_or(WpaError::RsnMismatch)?;
        if ap_rsn != self.ap_rsn {
            return Err(WpaError::RsnMismatch);
        }
        let (index, key) = gtk_kde(&plain)?;
        let tk = Zeroizing::new(ptk.tk().to_vec());
        let reply = KeyFrame {
            version: f.version,
            info: info::MIC | info::SECURE | info::PAIRWISE | info::VERSION_2,
            key_len: 0,
            replay: f.replay,
            nonce: [0; 32],
            rsc: [0; 8],
            mic: [0; 16],
            data: Vec::new(),
        };
        let reply = self.sign(reply)?;
        self.replay = Some(f.replay);
        if self.phase == Phase::Done {
            // Message 4 was lost: answer again, never install again.
            return Ok(Step::Send(reply));
        }
        self.phase = Phase::Done;
        self.gtk = key.clone();
        Ok(Step::Complete {
            reply,
            tk,
            group: GroupKey {
                index,
                key,
                rsc: f.rsc,
            },
        })
    }

    fn group_1(&mut self, f: &KeyFrame) -> Result<Step, WpaError> {
        if !f.has(info::ENCRYPTED | info::SECURE) || f.has(info::INSTALL) {
            return Err(WpaError::Unexpected);
        }
        let ptk = self.check(f)?;
        let plain = unwrap(ptk.kek(), &f.data)?;
        let (index, key) = gtk_kde(&plain)?;
        let reply = KeyFrame {
            version: f.version,
            info: info::MIC | info::SECURE | info::VERSION_2,
            key_len: 0,
            replay: f.replay,
            nonce: [0; 32],
            rsc: [0; 8],
            mic: [0; 16],
            data: Vec::new(),
        };
        let reply = self.sign(reply)?;
        self.replay = Some(f.replay);
        if key == self.gtk {
            return Ok(Step::Send(reply));
        }
        self.gtk = key.clone();
        Ok(Step::Rekey {
            reply,
            group: GroupKey {
                index,
                key,
                rsc: f.rsc,
            },
        })
    }
}

/// AES key unwrap (RFC 3394) of the key data.
fn unwrap(kek: &[u8], data: &[u8]) -> Result<Zeroizing<Vec<u8>>, WpaError> {
    if data.len() < 24 || !data.len().is_multiple_of(8) {
        return Err(WpaError::BadKeyData);
    }
    let kek: [u8; 16] = kek.try_into().map_err(|_| WpaError::BadKeyData)?;
    let mut out = Zeroizing::new(vec![0u8; data.len() - 8]);
    // RFC 3394 unwrap (the method is called `unwrap`; it returns a Result).
    KekAes128::unwrap(&KekAes128::from(kek), data, &mut out).map_err(|_| WpaError::BadKeyData)?;
    Ok(out)
}

/// The GTK KDE (00-0F-AC:1) of decrypted key data.
fn gtk_kde(plain: &[u8]) -> Result<(u8, Zeroizing<Vec<u8>>), WpaError> {
    let body = ie::elements(plain)
        .find(|(id, body)| {
            *id == ie::VENDOR && body.len() >= 4 && body[..4] == [0x00, 0x0f, 0xac, 0x01]
        })
        .map(|(_, body)| &body[4..])
        .ok_or(WpaError::BadKeyData)?;
    if body.len() != 2 + GTK_LEN {
        return Err(WpaError::BadKeyData);
    }
    let index = body[0] & 0x03;
    if index == 0 {
        return Err(WpaError::BadKeyData);
    }
    Ok((index, Zeroizing::new(body[2..].to_vec())))
}

#[cfg(test)]
#[path = "handshake_tests.rs"]
mod tests;
