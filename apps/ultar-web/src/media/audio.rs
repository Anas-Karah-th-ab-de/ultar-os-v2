//! The sound of a file, decoded in pure Rust (symphonia) and converted to the sound card's
//! format: 48 kHz, stereo, 16 bit (linear resampling, mono doubled, more channels mixed down).

use super::RATE;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The file's bytes, shared by the sound and the picture threads.
#[derive(Clone)]
pub struct Bytes(pub Arc<Vec<u8>>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Why the sound cannot be played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// No sound track this player can decode (or not a media file).
    Unsupported,
}

/// The decoded sound, chunk by chunk.
pub struct AudioStream {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    duration: Option<Duration>,
    resampler: Resampler,
}

impl AudioStream {
    /// The first sound track of `bytes` that can be decoded; `mime` helps to find the format.
    pub fn open(bytes: Bytes, mime: &str) -> Result<AudioStream, AudioError> {
        let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
        let mut hint = Hint::new();
        if !mime.is_empty() {
            hint.mime_type(mime);
        }
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                source,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|_| AudioError::Unsupported)?;
        let format = probed.format;
        let codecs = symphonia::default::get_codecs();
        let (track, decoder, params) = format
            .tracks()
            .iter()
            .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .find_map(|t| {
                let d = codecs
                    .make(&t.codec_params, &DecoderOptions::default())
                    .ok()?;
                Some((t.id, d, t.codec_params.clone()))
            })
            .ok_or(AudioError::Unsupported)?;
        let duration = params.n_frames.zip(params.time_base).map(|(n, tb)| {
            let t = tb.calc_time(n);
            Duration::from_secs(t.seconds) + Duration::from_secs_f64(t.frac)
        });
        Ok(AudioStream {
            format,
            decoder,
            track,
            duration,
            resampler: Resampler::default(),
        })
    }

    /// Length of the sound, when the file says.
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    /// The next chunk in the card's format (interleaved stereo samples); `None` at the end.
    /// A damaged packet is skipped.
    pub fn next_chunk(&mut self) -> Option<Vec<i16>> {
        loop {
            let packet = self.format.next_packet().ok()?;
            if packet.track_id() != self.track {
                continue;
            }
            let Ok(decoded) = self.decoder.decode(&packet) else {
                continue;
            };
            let spec = *decoded.spec();
            let channels = spec.channels.count().max(1);
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            let stereo = to_stereo(buf.samples(), channels);
            let out = self.resampler.run(&stereo, spec.rate);
            if !out.is_empty() {
                return Some(out);
            }
        }
    }
}

/// Interleaved samples of `channels` channels as interleaved stereo.
fn to_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() / channels * 2 + 2);
    for frame in samples.chunks_exact(channels) {
        let (l, r) = match frame {
            [m] => (*m, *m),
            [l, r] => (*l, *r),
            [l, r, rest @ ..] => {
                let mix = rest.iter().sum::<f32>() / rest.len() as f32 * 0.5;
                (l + mix, r + mix)
            }
            [] => (0.0, 0.0),
        };
        out.push(l);
        out.push(r);
    }
    out
}

/// Linear resampling of interleaved stereo to [`RATE`], across chunks.
#[derive(Default)]
struct Resampler {
    /// Position of the next output frame in input frames, counted from `last`.
    pos: f64,
    /// The last input frame of the previous chunk.
    last: Option<(f32, f32)>,
}

impl Resampler {
    fn run(&mut self, stereo: &[f32], rate: u32) -> Vec<i16> {
        let frames: Vec<(f32, f32)> = stereo.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        if rate == RATE || rate == 0 {
            self.last = frames.last().copied();
            return frames
                .iter()
                .flat_map(|&(l, r)| [sample(l), sample(r)])
                .collect();
        }
        let step = f64::from(rate) / f64::from(RATE);
        // input frames with the last one of the previous chunk in front (index 0)
        let prev = self
            .last
            .unwrap_or(frames.first().copied().unwrap_or((0.0, 0.0)));
        let at = |i: usize| {
            if i == 0 {
                prev
            } else {
                frames.get(i - 1).copied().unwrap_or(prev)
            }
        };
        let mut out = Vec::with_capacity((frames.len() as f64 / step) as usize * 2 + 4);
        while self.pos < frames.len() as f64 {
            let i = self.pos.floor() as usize;
            let f = (self.pos - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push(sample(a.0 + (b.0 - a.0) * f));
            out.push(sample(a.1 + (b.1 - a.1) * f));
            self.pos += step;
        }
        self.pos -= frames.len() as f64;
        self.last = frames.last().copied().or(self.last);
        out
    }
}

fn sample(v: f32) -> i16 {
    (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A WAV file of `seconds` of a 440 Hz tone at `rate`, mono.
    pub(crate) fn wav(rate: u32, seconds: f32) -> Vec<u8> {
        let n = (rate as f32 * seconds) as u32;
        let mut out = Vec::new();
        let data = n * 2;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for i in 0..n {
            let v = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 8000.0;
            out.extend_from_slice(&(v as i16).to_le_bytes());
        }
        out
    }

    #[test]
    fn a_wav_file_becomes_48k_stereo() {
        let mut s = AudioStream::open(Bytes(Arc::new(wav(16_000, 0.5))), "audio/wav").unwrap();
        let d = s.duration().unwrap();
        assert!(
            d >= Duration::from_millis(490) && d <= Duration::from_millis(510),
            "{d:?}"
        );
        let mut samples = 0;
        while let Some(chunk) = s.next_chunk() {
            assert_eq!(chunk.len() % 2, 0);
            samples += chunk.len();
        }
        // half a second at 48 kHz, two channels (within one frame of rounding)
        assert!((47_990..=48_010).contains(&samples), "{samples}");
    }

    #[test]
    fn not_a_media_file() {
        let r = AudioStream::open(Bytes(Arc::new(b"<html>".to_vec())), "text/html");
        assert_eq!(r.err(), Some(AudioError::Unsupported));
    }

    #[test]
    fn channels_become_stereo() {
        assert_eq!(to_stereo(&[0.5], 1), [0.5, 0.5]);
        assert_eq!(to_stereo(&[0.1, 0.2, 0.4, 0.4], 4).len(), 2);
    }
}
