//! Linear PCM16 helpers and the WAV container for recordings (mono, 16 kHz).
//!
//! The detector works on `f32` samples in `-1.0..1.0`; the bus and the speech backends
//! carry `i16` PCM. Enrolment clips and the request are short, so they are kept in memory as
//! `Vec<f32>` and only handed to the STT process as a WAV file on disk.

/// Smallest root-mean-square in `-1.0..1.0` that still counts as speech.
pub const SILENCE: f32 = 0.01;

/// Converts linear PCM16 to `f32` samples in `-1.0..1.0`.
pub fn pcm16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| f32::from(s) / 32768.0).collect()
}

/// Converts `f32` samples in `-1.0..1.0` back to linear PCM16.
pub fn f32_to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// Root-mean-square of a slice; `0.0` for an empty slice.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Whether the slice is below the speech threshold.
pub fn is_silent(samples: &[f32]) -> bool {
    rms(samples) < SILENCE
}

/// A decoded WAV recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    /// Sample rate in Hertz.
    pub rate: u32,
    /// Channel count; decoded to mono by averaging.
    pub channels: u16,
    /// The (mono) samples.
    pub samples: Vec<i16>,
}

/// Decodes a 16-bit PCM WAV into mono samples. Returns `None` for anything that is not a
/// plain PCM WAV or has a format this service does not need.
pub fn decode_wav(bytes: &[u8]) -> Option<Wav> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut channels = 0u16;
    let mut rate = 0u32;
    let mut bits = 0u16;
    let mut samples = Vec::new();
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        let end = body.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        match id {
            b"fmt " => {
                if size < 16 {
                    return None;
                }
                let format = u16::from_le_bytes(bytes[body..body + 2].try_into().ok()?);
                channels = u16::from_le_bytes(bytes[body + 2..body + 4].try_into().ok()?);
                rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().ok()?);
                bits = u16::from_le_bytes(bytes[body + 14..body + 16].try_into().ok()?);
                if format != 1 || bits != 16 || channels == 0 {
                    return None;
                }
            }
            b"data" => {
                let frames = size / 2;
                let channels = usize::from(channels);
                for i in (0..frames).step_by(channels) {
                    let mut sum = 0i32;
                    let mut n = 0i32;
                    for c in 0..channels {
                        let at = body + (i + c) * 2;
                        if at + 2 > end {
                            return None;
                        }
                        sum += i32::from(i16::from_le_bytes(bytes[at..at + 2].try_into().ok()?));
                        n += 1;
                    }
                    samples.push((sum / n) as i16);
                }
            }
            _ => {}
        }
        pos = end + (end % 2); // the chunks are word-aligned
    }
    Some(Wav {
        rate,
        channels,
        samples,
    })
}

/// Encodes mono 16-bit PCM as a WAV file (for the speech-to-text process and the test
/// fixtures).
pub fn encode_wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_and_rms_round_trip() {
        let pcm = [-32768i16, -1, 0, 1, 32767];
        let f = pcm16_to_f32(&pcm);
        assert!(f.iter().all(|s| *s >= -1.0 && *s <= 1.0));
        let back = f32_to_pcm16(&f);
        assert!(back[0] <= -32767 && back[4] >= 32766);
        assert!(is_silent(&[0.0f32; 160]));
        assert!(!is_silent(&vec![0.2f32; 160]));
    }

    #[test]
    fn wav_writes_and_reads_back() {
        let samples: Vec<i16> = (0..800).map(|i| ((i as i32 * 37) % 32000) as i16).collect();
        let bytes = encode_wav(16_000, &samples);
        let wav = decode_wav(&bytes).unwrap();
        assert_eq!(wav.rate, 16_000);
        assert_eq!(wav.channels, 1);
        assert_eq!(wav.samples, samples);
    }

    #[test]
    fn a_cut_or_wrong_wav_is_rejected() {
        let samples = vec![0i16; 400];
        let bytes = encode_wav(16_000, &samples);
        for cut in 0..bytes.len() {
            assert!(decode_wav(&bytes[..cut]).is_none(), "cut {cut}");
        }
        assert!(decode_wav(b"not a wav at all").is_none());
    }
}