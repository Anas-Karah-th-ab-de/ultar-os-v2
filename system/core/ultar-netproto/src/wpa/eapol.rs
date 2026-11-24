//! EAPOL-Key frames (IEEE 802.11-2016 12.7.2, IEEE 802.1X-2010 11.3): read
//! hostile bytes, write the supplicant's answers, the MIC (HMAC-SHA1-128,
//! key descriptor version 2) checked in constant time.

use super::keys::hmac_sha1;
use super::WpaError;
use subtle::ConstantTimeEq;

/// EAPOL packet type of key frames.
pub const TYPE_KEY: u8 = 3;
/// Key descriptor type of RSN.
pub const DESCRIPTOR_RSN: u8 = 2;
/// Bytes before the key data (EAPOL header 4 + descriptor 95).
pub const HEADER: usize = 99;
/// Offset of the MIC in the frame.
const MIC_AT: usize = 81;
/// Longest key data accepted.
pub const MAX_KEY_DATA: usize = 1024;

/// Key information bits.
pub mod info {
    /// Descriptor version mask.
    pub const VERSION: u16 = 0x0007;
    /// Version 2: HMAC-SHA1-128 MIC, AES key wrap.
    pub const VERSION_2: u16 = 2;
    /// Pairwise key.
    pub const PAIRWISE: u16 = 1 << 3;
    /// Install the key.
    pub const INSTALL: u16 = 1 << 6;
    /// The authenticator expects an answer.
    pub const ACK: u16 = 1 << 7;
    /// A MIC is present.
    pub const MIC: u16 = 1 << 8;
    /// Keys are installed (secure).
    pub const SECURE: u16 = 1 << 9;
    /// Error (MIC failure report).
    pub const ERROR: u16 = 1 << 10;
    /// Request.
    pub const REQUEST: u16 = 1 << 11;
    /// The key data is encrypted.
    pub const ENCRYPTED: u16 = 1 << 12;
}

/// A parsed EAPOL-Key frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyFrame {
    /// EAPOL protocol version of the frame (answered with the same).
    pub version: u8,
    /// Key information.
    pub info: u16,
    /// Key length.
    pub key_len: u16,
    /// Replay counter.
    pub replay: u64,
    /// Nonce.
    pub nonce: [u8; 32],
    /// Receive sequence counter of the group key.
    pub rsc: [u8; 8],
    /// MIC.
    pub mic: [u8; 16],
    /// Key data (still encrypted when [`info::ENCRYPTED`] is set).
    pub data: Vec<u8>,
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

impl KeyFrame {
    /// Reads a whole EAPOL frame. Trailing bytes after the declared length
    /// (Ethernet padding) are ignored.
    pub fn parse(frame: &[u8]) -> Result<KeyFrame, WpaError> {
        if frame.len() < HEADER || frame[1] != TYPE_KEY || frame[4] != DESCRIPTOR_RSN {
            return Err(WpaError::BadFrame);
        }
        let body_len = usize::from(be16(&frame[2..4]));
        let data_len = usize::from(be16(&frame[97..99]));
        if body_len < HEADER - 4
            || frame.len() < 4 + body_len
            || body_len != HEADER - 4 + data_len
            || data_len > MAX_KEY_DATA
        {
            return Err(WpaError::BadFrame);
        }
        let mut f = KeyFrame {
            version: frame[0],
            info: be16(&frame[5..7]),
            key_len: be16(&frame[7..9]),
            replay: u64::from_be_bytes(frame[9..17].try_into().map_err(|_| WpaError::BadFrame)?),
            nonce: [0; 32],
            rsc: [0; 8],
            mic: [0; 16],
            data: frame[HEADER..HEADER + data_len].to_vec(),
        };
        f.nonce.copy_from_slice(&frame[17..49]);
        f.rsc.copy_from_slice(&frame[65..73]);
        f.mic.copy_from_slice(&frame[MIC_AT..MIC_AT + 16]);
        Ok(f)
    }

    /// The frame's bytes (with its MIC field as it is).
    pub fn to_bytes(&self) -> Vec<u8> {
        let body_len = (HEADER - 4 + self.data.len()) as u16;
        let mut b = Vec::with_capacity(HEADER + self.data.len());
        b.extend_from_slice(&[self.version, TYPE_KEY]);
        b.extend_from_slice(&body_len.to_be_bytes());
        b.push(DESCRIPTOR_RSN);
        b.extend_from_slice(&self.info.to_be_bytes());
        b.extend_from_slice(&self.key_len.to_be_bytes());
        b.extend_from_slice(&self.replay.to_be_bytes());
        b.extend_from_slice(&self.nonce);
        b.extend_from_slice(&[0; 16]); // EAPOL-Key IV
        b.extend_from_slice(&self.rsc);
        b.extend_from_slice(&[0; 8]); // reserved
        b.extend_from_slice(&self.mic);
        b.extend_from_slice(&(self.data.len() as u16).to_be_bytes());
        b.extend_from_slice(&self.data);
        b
    }

    /// The MIC of this frame under `kck` (computed with a zero MIC field).
    pub fn compute_mic(&self, kck: &[u8]) -> [u8; 16] {
        let mut zeroed = self.clone();
        zeroed.mic = [0; 16];
        let full = hmac_sha1(kck, &[&zeroed.to_bytes()]);
        let mut mic = [0u8; 16];
        mic.copy_from_slice(&full[..16]);
        mic
    }

    /// Sets the MIC under `kck` and returns the bytes to send.
    pub fn sign(mut self, kck: &[u8]) -> Vec<u8> {
        self.mic = self.compute_mic(kck);
        self.to_bytes()
    }

    /// The MIC is right (constant-time comparison).
    pub fn mic_ok(&self, kck: &[u8]) -> bool {
        self.compute_mic(kck).ct_eq(&self.mic).into()
    }

    /// Bits of the key information.
    pub fn has(&self, bits: u16) -> bool {
        self.info & bits == bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> KeyFrame {
        KeyFrame {
            version: 2,
            info: info::ACK | info::PAIRWISE | info::VERSION_2,
            key_len: 16,
            replay: 7,
            nonce: [5; 32],
            rsc: [1, 0, 0, 0, 0, 0, 0, 0],
            mic: [0; 16],
            data: vec![0xdd, 0],
        }
    }

    #[test]
    fn round_trip_and_mic() {
        let f = sample();
        let bytes = f.to_bytes();
        assert_eq!(bytes.len(), HEADER + 2);
        assert_eq!(KeyFrame::parse(&bytes).unwrap(), f);
        let mut padded = bytes.clone();
        padded.extend_from_slice(&[0; 20]);
        assert_eq!(KeyFrame::parse(&padded).unwrap(), f, "Ethernet padding");
        let signed = KeyFrame::parse(&f.clone().sign(&[9; 16])).unwrap();
        assert!(signed.mic_ok(&[9; 16]));
        assert!(!signed.mic_ok(&[8; 16]));
        assert!(signed.has(info::ACK | info::PAIRWISE) && !signed.has(info::MIC));
    }

    #[test]
    fn hostile_frames_are_refused() {
        let good = sample().to_bytes();
        let mut cases: Vec<Vec<u8>> =
            vec![good[..HEADER - 1].to_vec(), good[..good.len() - 1].to_vec()];
        let mut t = good.clone();
        t[1] = 0;
        cases.push(t);
        let mut t = good.clone();
        t[4] = 254;
        cases.push(t);
        let mut t = good.clone();
        t[3] = t[3].wrapping_add(1);
        cases.push(t);
        let mut t = good.clone();
        t[98] = 200;
        cases.push(t);
        for c in cases {
            assert_eq!(KeyFrame::parse(&c), Err(WpaError::BadFrame));
        }
        // Fixed-seed fuzz: every cut and changed byte parses or is refused.
        let mut seed = 0x1234_5678u32;
        for _ in 0..5000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut t = good.clone();
            let at = seed as usize % t.len();
            t[at] = (seed >> 8) as u8;
            t.truncate(at + (seed as usize >> 20) % (t.len() - at + 1));
            let _ = KeyFrame::parse(&t);
        }
    }
}
