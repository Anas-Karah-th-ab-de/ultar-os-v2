//! Audio class: outputs, inputs, streams, volume, mute and routing.

use crate::error::HalResult;
use crate::types::{EndpointId, Level};

/// Direction of sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioDirection {
    /// Playing.
    Output,
    /// Recording.
    Input,
}

/// Kind of an audio endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioEndpointKind {
    /// Loudspeaker.
    Speaker,
    /// Earpiece for calls.
    Earpiece,
    /// Wired headphones or headset.
    Headphones,
    /// Digital output on a display cable.
    DisplayAudio,
    /// Bluetooth audio device.
    Bluetooth,
    /// USB audio device.
    Usb,
    /// Built-in microphone.
    Microphone,
    /// Microphone of a headset.
    HeadsetMicrophone,
}

/// An output or input of the device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioEndpoint {
    /// Id of the endpoint.
    pub id: EndpointId,
    /// Kind of endpoint.
    pub kind: AudioEndpointKind,
    /// Playing or recording.
    pub direction: AudioDirection,
    /// True if something is plugged in or the endpoint is always there.
    pub available: bool,
}

/// Encoding of one sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    /// Signed 16 bit, little endian.
    S16,
    /// Signed 24 bit in 32 bit, little endian.
    S24,
    /// Signed 32 bit, little endian.
    S32,
    /// 32 bit float.
    F32,
}

impl SampleFormat {
    /// Bytes one sample occupies.
    pub const fn bytes(self) -> u32 {
        match self {
            SampleFormat::S16 => 2,
            SampleFormat::S24 | SampleFormat::S32 | SampleFormat::F32 => 4,
        }
    }
}

/// Format of an audio stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioFormat {
    /// Samples per second.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u8,
    /// Encoding of a sample.
    pub sample: SampleFormat,
}

impl AudioFormat {
    /// 48 kHz, stereo, 16 bit: the format every backend must accept for output.
    pub const DEFAULT: AudioFormat = AudioFormat {
        sample_rate: 48_000,
        channels: 2,
        sample: SampleFormat::S16,
    };

    /// Bytes of one frame (one sample for every channel).
    pub const fn bytes_per_frame(&self) -> u32 {
        self.sample.bytes() * self.channels as u32
    }

    /// Bytes of one second of sound.
    pub const fn bytes_per_second(&self) -> u64 {
        self.bytes_per_frame() as u64 * self.sample_rate as u64
    }

    /// True if rate and channel count are usable at all.
    pub const fn is_valid(&self) -> bool {
        self.sample_rate >= 8_000 && self.sample_rate <= 384_000 && self.channels >= 1
    }
}

/// An open playback or recording stream. Dropping it closes the stream.
pub trait AudioStream: Send {
    /// Format the stream runs with.
    fn format(&self) -> AudioFormat;
    /// Direction of the stream.
    fn direction(&self) -> AudioDirection;
    /// Plays whole frames from `data`; returns the number of bytes taken.
    fn write(&mut self, data: &[u8]) -> HalResult<usize>;
    /// Records whole frames into `buffer`; returns the number of bytes filled.
    fn read(&mut self, buffer: &mut [u8]) -> HalResult<usize>;
    /// Blocks until everything written has been played.
    fn drain(&mut self) -> HalResult<()>;
}

/// Sound hardware of a backend.
pub trait Audio: Send + Sync {
    /// Outputs of the device.
    fn outputs(&self) -> HalResult<Vec<AudioEndpoint>>;
    /// Inputs of the device.
    fn inputs(&self) -> HalResult<Vec<AudioEndpoint>>;
    /// Opens a stream on an endpoint; the direction follows from the endpoint.
    fn open_stream(
        &self,
        endpoint: EndpointId,
        format: AudioFormat,
    ) -> HalResult<Box<dyn AudioStream>>;
    /// Volume of an endpoint.
    fn volume(&self, endpoint: EndpointId) -> HalResult<Level>;
    /// Sets the volume of an endpoint.
    fn set_volume(&self, endpoint: EndpointId, level: Level) -> HalResult<()>;
    /// True if the endpoint is muted.
    fn is_muted(&self, endpoint: EndpointId) -> HalResult<bool>;
    /// Mutes or unmutes an endpoint.
    fn set_muted(&self, endpoint: EndpointId, muted: bool) -> HalResult<()>;
    /// Endpoint sound currently goes to (or comes from) for a direction.
    fn route(&self, direction: AudioDirection) -> HalResult<EndpointId>;
    /// Sends sound of the endpoint's direction through this endpoint.
    fn set_route(&self, endpoint: EndpointId) -> HalResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_sizes() {
        assert_eq!(AudioFormat::DEFAULT.bytes_per_frame(), 4);
        assert_eq!(AudioFormat::DEFAULT.bytes_per_second(), 192_000);
        let surround = AudioFormat {
            sample_rate: 96_000,
            channels: 6,
            sample: SampleFormat::F32,
        };
        assert_eq!(surround.bytes_per_frame(), 24);
        assert_eq!(SampleFormat::S24.bytes(), 4);
        assert_eq!(SampleFormat::S32.bytes(), 4);
    }

    #[test]
    fn format_validity() {
        assert!(AudioFormat::DEFAULT.is_valid());
        let mut format = AudioFormat::DEFAULT;
        format.channels = 0;
        assert!(!format.is_valid());
        format.channels = 1;
        format.sample_rate = 7_999;
        assert!(!format.is_valid());
        format.sample_rate = 384_001;
        assert!(!format.is_valid());
        format.sample_rate = 8_000;
        assert!(format.is_valid());
    }
}
