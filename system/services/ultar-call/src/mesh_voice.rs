//! The voice of one call that is carried over the mesh: sound card or line to
//! `mesh.call_audio` and back, 48 kHz mono in frames of 20 ms, in order, with a jitter
//! buffer and loss concealment on the way in (see [`crate::mesh_relay`]).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::mesh_jitter::Jitter;
use crate::mesh_tap::{MeshTap, FRAME, FRAME_BYTES};
use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use base64::Engine;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh_calls::{CallAudio, CallAudioIn};
use ultar_bus::Client;

/// How long the mesh service may take to take one frame.
const MESH_WAIT: Duration = Duration::from_millis(300);
/// How long one frame lasts.
const FRAME_TIME: Duration = Duration::from_millis(20);
/// Frames sent or played per step at most (a step is a tick of the service).
const BURST: usize = 8;
/// A playout clock this far behind starts again (a stalled service must not burst).
const MAX_LAG: Duration = Duration::from_millis(100);

/// The voice of one call.
pub(crate) struct Voice {
    /// The offer (the call's id on the mesh).
    pub(crate) id: u64,
    /// The other device.
    pub(crate) device: String,
    seq_out: u64,
    jitter: Jitter,
    next_play: Instant,
}

/// The samples of one frame; `None` for anything but exactly one 20 ms frame.
pub(crate) fn decode(pcm: &str) -> Option<Vec<i16>> {
    let bytes = B64.decode(pcm).ok()?;
    if bytes.len() != FRAME_BYTES {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect(),
    )
}

fn encode(frame: &[i16]) -> String {
    let bytes: Vec<u8> = frame
        .iter()
        .take(FRAME)
        .flat_map(|s| s.to_le_bytes())
        .collect();
    B64.encode(bytes)
}

impl Voice {
    pub(crate) fn new(id: u64, device: String, now: Instant) -> Voice {
        Voice {
            id,
            device,
            seq_out: 0,
            jitter: Jitter::default(),
            next_play: now,
        }
    }

    /// A frame arrived from the other device.
    pub(crate) fn hear(&mut self, audio: &CallAudio) {
        if audio.id != self.id {
            return;
        }
        if let Some(samples) = decode(&audio.pcm) {
            self.jitter.put(audio.seq, samples);
        }
    }

    /// Sends what the microphone has (when `send`) and plays what is due (when `play`).
    pub(crate) fn pump(
        &mut self,
        bus: &Client,
        tap: &mut dyn MeshTap,
        send: bool,
        play: bool,
        now: Instant,
    ) {
        for _ in 0..BURST {
            let Some(frame) = tap.capture() else { break };
            if !send {
                continue;
            }
            let out = CallAudioIn {
                device: self.device.clone(),
                id: self.id,
                seq: self.seq_out,
                pcm: encode(&frame),
            };
            self.seq_out += 1;
            let _ = bus.call(&out, MESH_WAIT);
        }
        // One frame per 20 ms that went by, never a long burst.
        if now.saturating_duration_since(self.next_play) > MAX_LAG {
            self.next_play = now;
        }
        let mut due = 0;
        while self.next_play <= now && due < BURST {
            let frame = self.jitter.take();
            self.next_play += FRAME_TIME;
            due += 1;
            if play {
                tap.play(&frame);
            }
        }
    }
}
