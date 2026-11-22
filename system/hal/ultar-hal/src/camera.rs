//! Camera class: cameras, their formats and frame streams.

use crate::error::HalResult;
use crate::types::{CameraId, PixelFormat, Size};

/// Where a camera looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CameraFacing {
    /// Towards the user.
    Front,
    /// Away from the user.
    Back,
    /// Plugged-in camera.
    External,
}

/// Description of a camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CameraInfo {
    /// Id of the camera.
    pub id: CameraId,
    /// Where it looks.
    pub facing: CameraFacing,
    /// True if the camera has a light that can serve as a torch.
    pub has_torch: bool,
}

/// One stream format of a camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CameraFormat {
    /// Resolution.
    pub size: Size,
    /// Pixel format.
    pub format: PixelFormat,
    /// Frames per second.
    pub frames_per_second: u32,
}

/// One captured frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraFrame {
    /// Resolution.
    pub size: Size,
    /// Pixel format.
    pub format: PixelFormat,
    /// Running number of the frame within its stream.
    pub sequence: u64,
    /// Capture time in microseconds on the monotonic clock.
    pub time_us: u64,
    /// The frame bytes.
    pub data: Vec<u8>,
}

/// An open camera stream. Dropping it stops the camera.
pub trait CameraStream: Send {
    /// Format the stream delivers.
    fn format(&self) -> CameraFormat;
    /// Waits up to `timeout_ms` for the next frame.
    fn next_frame(&mut self, timeout_ms: u32) -> HalResult<CameraFrame>;
}

/// Cameras of a backend.
pub trait Camera: Send + Sync {
    /// Every camera.
    fn list(&self) -> HalResult<Vec<CameraInfo>>;
    /// Formats a camera offers.
    fn formats(&self, id: CameraId) -> HalResult<Vec<CameraFormat>>;
    /// Starts a stream in one of the offered formats.
    fn open(&self, id: CameraId, format: CameraFormat) -> HalResult<Box<dyn CameraStream>>;
    /// Switches the torch of a camera on or off.
    fn set_torch(&self, id: CameraId, on: bool) -> HalResult<()>;
}
