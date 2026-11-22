//! Mock sound hardware.

use super::cell::MockCell;
use crate::audio::{
    Audio, AudioDirection, AudioEndpoint, AudioEndpointKind, AudioFormat, AudioStream,
};
use crate::error::{HalError, HalResult};
use crate::types::{EndpointId, Level};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
struct Endpoint {
    info: AudioEndpoint,
    volume: Level,
    muted: bool,
}

#[derive(Debug, Default)]
struct AudioState {
    endpoints: Vec<Endpoint>,
    output_route: Option<EndpointId>,
    input_route: Option<EndpointId>,
    next_id: u32,
}

impl AudioState {
    fn endpoint(&mut self, id: EndpointId) -> HalResult<&mut Endpoint> {
        self.endpoints
            .iter_mut()
            .find(|endpoint| endpoint.info.id == id)
            .ok_or(HalError::NotFound)
    }

    fn list(&self, direction: AudioDirection) -> Vec<AudioEndpoint> {
        self.endpoints
            .iter()
            .filter(|endpoint| endpoint.info.direction == direction)
            .map(|endpoint| endpoint.info.clone())
            .collect()
    }
}

/// Counters shared between the mock and the streams it opened.
#[derive(Debug, Default)]
struct StreamLog {
    open_streams: AtomicUsize,
    bytes_played: AtomicU64,
    bytes_recorded: AtomicU64,
}

/// Scriptable sound hardware. Starts without endpoints.
#[derive(Debug, Default)]
pub struct MockAudio {
    cell: MockCell<AudioState>,
    log: Arc<StreamLog>,
}

impl MockAudio {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Adds an endpoint at half volume. The first endpoint of a direction becomes
    /// the route of that direction.
    pub fn add_endpoint(&self, kind: AudioEndpointKind, direction: AudioDirection) -> EndpointId {
        let mut state = self.cell.state();
        let id = EndpointId(state.next_id);
        state.next_id += 1;
        state.endpoints.push(Endpoint {
            info: AudioEndpoint {
                id,
                kind,
                direction,
                available: true,
            },
            volume: Level::from_fraction(0.5),
            muted: false,
        });
        let route = match direction {
            AudioDirection::Output => &mut state.output_route,
            AudioDirection::Input => &mut state.input_route,
        };
        route.get_or_insert(id);
        id
    }

    /// Marks an endpoint as plugged in or pulled out.
    pub fn set_available(&self, id: EndpointId, available: bool) -> HalResult<()> {
        self.cell.state().endpoint(id)?.info.available = available;
        Ok(())
    }

    /// Number of streams currently open.
    pub fn open_streams(&self) -> usize {
        self.log.open_streams.load(Ordering::SeqCst)
    }

    /// Bytes written to output streams so far.
    pub fn bytes_played(&self) -> u64 {
        self.log.bytes_played.load(Ordering::SeqCst)
    }

    /// Bytes read from input streams so far.
    pub fn bytes_recorded(&self) -> u64 {
        self.log.bytes_recorded.load(Ordering::SeqCst)
    }
}

struct MockStream {
    format: AudioFormat,
    direction: AudioDirection,
    log: Arc<StreamLog>,
    position: u64,
}

impl MockStream {
    fn whole_frames(&self, length: usize) -> usize {
        let frame = self.format.bytes_per_frame() as usize;
        length - length % frame.max(1)
    }
}

impl AudioStream for MockStream {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn direction(&self) -> AudioDirection {
        self.direction
    }

    fn write(&mut self, data: &[u8]) -> HalResult<usize> {
        if self.direction != AudioDirection::Output {
            return Err(HalError::NotSupported);
        }
        let taken = self.whole_frames(data.len());
        self.log
            .bytes_played
            .fetch_add(taken as u64, Ordering::SeqCst);
        Ok(taken)
    }

    /// Fills the buffer with a counting byte pattern (0, 1, 2 … wrapping).
    fn read(&mut self, buffer: &mut [u8]) -> HalResult<usize> {
        if self.direction != AudioDirection::Input {
            return Err(HalError::NotSupported);
        }
        let filled = self.whole_frames(buffer.len());
        for byte in buffer.iter_mut().take(filled) {
            *byte = (self.position % 256) as u8;
            self.position += 1;
        }
        self.log
            .bytes_recorded
            .fetch_add(filled as u64, Ordering::SeqCst);
        Ok(filled)
    }

    fn drain(&mut self) -> HalResult<()> {
        Ok(())
    }
}

impl Drop for MockStream {
    fn drop(&mut self) {
        self.log.open_streams.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Audio for MockAudio {
    fn outputs(&self) -> HalResult<Vec<AudioEndpoint>> {
        self.cell
            .call(|state| Ok(state.list(AudioDirection::Output)))
    }

    fn inputs(&self) -> HalResult<Vec<AudioEndpoint>> {
        self.cell
            .call(|state| Ok(state.list(AudioDirection::Input)))
    }

    fn open_stream(
        &self,
        endpoint: EndpointId,
        format: AudioFormat,
    ) -> HalResult<Box<dyn AudioStream>> {
        self.cell.call(|state| {
            let info = &state.endpoint(endpoint)?.info;
            if !format.is_valid() {
                return Err(HalError::InvalidArgument);
            }
            if !info.available {
                return Err(HalError::Disconnected);
            }
            self.log.open_streams.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(MockStream {
                format,
                direction: info.direction,
                log: self.log.clone(),
                position: 0,
            }) as Box<dyn AudioStream>)
        })
    }

    fn volume(&self, endpoint: EndpointId) -> HalResult<Level> {
        self.cell.call(|state| Ok(state.endpoint(endpoint)?.volume))
    }

    fn set_volume(&self, endpoint: EndpointId, level: Level) -> HalResult<()> {
        self.cell.call(|state| {
            state.endpoint(endpoint)?.volume = level;
            Ok(())
        })
    }

    fn is_muted(&self, endpoint: EndpointId) -> HalResult<bool> {
        self.cell.call(|state| Ok(state.endpoint(endpoint)?.muted))
    }

    fn set_muted(&self, endpoint: EndpointId, muted: bool) -> HalResult<()> {
        self.cell.call(|state| {
            state.endpoint(endpoint)?.muted = muted;
            Ok(())
        })
    }

    fn route(&self, direction: AudioDirection) -> HalResult<EndpointId> {
        self.cell.call(|state| {
            let route = match direction {
                AudioDirection::Output => state.output_route,
                AudioDirection::Input => state.input_route,
            };
            route.ok_or(HalError::NotFound)
        })
    }

    fn set_route(&self, endpoint: EndpointId) -> HalResult<()> {
        self.cell.call(|state| {
            let info = state.endpoint(endpoint)?.info.clone();
            if !info.available {
                return Err(HalError::Disconnected);
            }
            match info.direction {
                AudioDirection::Output => state.output_route = Some(endpoint),
                AudioDirection::Input => state.input_route = Some(endpoint),
            }
            Ok(())
        })
    }
}
