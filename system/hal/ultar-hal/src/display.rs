//! Display class: outputs, modes, presenting frames, brightness, rotation, hotplug.
//! Also the renderer capability query.

use crate::error::{HalError, HalResult};
use crate::types::{DisplayId, Level, PixelFormat, Rect, Rotation, Size};
use ultar_profile::DisplayGeometry;

/// One video mode of a display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DisplayMode {
    /// Resolution in physical pixels.
    pub size: Size,
    /// Refresh rate in thousandths of a hertz (60 Hz = 60000).
    pub refresh_millihertz: u32,
}

impl DisplayMode {
    /// Creates a mode from resolution and refresh rate in whole hertz.
    pub const fn new(width: u32, height: u32, refresh_hz: u32) -> Self {
        DisplayMode {
            size: Size::new(width, height),
            refresh_millihertz: refresh_hz * 1000,
        }
    }
}

/// Static description of a connected display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    /// Id valid while the display stays connected.
    pub id: DisplayId,
    /// Connector name such as `eDP-1` or `HDMI-A-1` (technical, not for people).
    pub connector: String,
    /// True for the built-in panel, false for anything plugged in.
    pub internal: bool,
    /// Physical size in millimetres; zero if the display does not report it.
    pub physical_mm: Size,
    /// Mode the display prefers.
    pub preferred_mode: DisplayMode,
}

impl DisplayInfo {
    /// Geometry of this display at the given mode, in the form the device profile stores.
    pub fn geometry(&self, mode: DisplayMode) -> DisplayGeometry {
        DisplayGeometry::from_physical(
            mode.size.width,
            mode.size.height,
            self.physical_mm.width,
            self.physical_mm.height,
        )
    }
}

/// Something changed about the connected displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayEvent {
    /// A display was connected.
    Connected(DisplayId),
    /// A display was removed.
    Disconnected(DisplayId),
    /// The mode of a display changed.
    ModeChanged(DisplayId),
}

/// A borrowed frame handed to [`Display::present`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameView<'a> {
    size: Size,
    stride: u32,
    format: PixelFormat,
    data: &'a [u8],
}

impl<'a> FrameView<'a> {
    /// Wraps pixel data. `stride` is the number of bytes from one row to the next.
    /// Fails with [`HalError::InvalidArgument`] if the format has no fixed pixel size,
    /// the stride is shorter than a row or the buffer is too small.
    pub fn new(size: Size, stride: u32, format: PixelFormat, data: &'a [u8]) -> HalResult<Self> {
        let bytes_per_pixel = format.bytes_per_pixel().ok_or(HalError::InvalidArgument)?;
        let row_bytes = u64::from(size.width) * u64::from(bytes_per_pixel);
        if size.is_empty() || u64::from(stride) < row_bytes {
            return Err(HalError::InvalidArgument);
        }
        let needed = u64::from(stride) * u64::from(size.height - 1) + row_bytes;
        if (data.len() as u64) < needed {
            return Err(HalError::InvalidArgument);
        }
        Ok(FrameView {
            size,
            stride,
            format,
            data,
        })
    }

    /// Size of the frame in pixels.
    pub const fn size(&self) -> Size {
        self.size
    }

    /// Bytes from one row to the next.
    pub const fn stride(&self) -> u32 {
        self.stride
    }

    /// Pixel format.
    pub const fn format(&self) -> PixelFormat {
        self.format
    }

    /// The pixel bytes.
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }
}

/// Display outputs of a backend.
pub trait Display: Send + Sync {
    /// Connected displays, the primary one first.
    fn displays(&self) -> HalResult<Vec<DisplayInfo>>;
    /// Modes a display offers.
    fn modes(&self, id: DisplayId) -> HalResult<Vec<DisplayMode>>;
    /// Mode a display currently runs.
    fn current_mode(&self, id: DisplayId) -> HalResult<DisplayMode>;
    /// Switches a display to one of its modes.
    fn set_mode(&self, id: DisplayId, mode: DisplayMode) -> HalResult<()>;
    /// Shows a frame. `damage` lists the areas that changed since the last frame;
    /// an empty list means the whole frame changed.
    fn present(&self, id: DisplayId, frame: &FrameView<'_>, damage: &[Rect]) -> HalResult<()>;
    /// Current backlight level.
    fn brightness(&self, id: DisplayId) -> HalResult<Level>;
    /// Sets the backlight level.
    fn set_brightness(&self, id: DisplayId, level: Level) -> HalResult<()>;
    /// Current rotation.
    fn rotation(&self, id: DisplayId) -> HalResult<Rotation>;
    /// Sets the rotation.
    fn set_rotation(&self, id: DisplayId, rotation: Rotation) -> HalResult<()>;
    /// Returns and clears the hotplug and mode events since the last call.
    fn poll_events(&self) -> HalResult<Vec<DisplayEvent>>;
    /// The display's own identification data (EDID bytes), if it sent any.
    /// Backends that cannot read it answer `None` (the default).
    fn identification(&self, id: DisplayId) -> HalResult<Option<Vec<u8>>> {
        let _ = id;
        Ok(None)
    }
}

/// How frames are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RendererKind {
    /// Drawn by the CPU.
    Software,
    /// Drawn by a GPU.
    Gpu,
}

/// Capability query for rendering.
pub trait Renderer: Send + Sync {
    /// Software or GPU.
    fn kind(&self) -> RendererKind;
    /// Largest surface side the renderer handles, in pixels.
    fn max_surface_size(&self) -> u32;
    /// True if the renderer can produce frames in this format.
    fn supports_format(&self, format: PixelFormat) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_view_validation() {
        let data = vec![0u8; 4 * 4 * 2];
        let size = Size::new(4, 2);
        let frame = FrameView::new(size, 16, PixelFormat::Xrgb8888, &data).unwrap();
        assert_eq!(frame.size(), size);
        assert_eq!(frame.stride(), 16);
        assert_eq!(frame.format(), PixelFormat::Xrgb8888);
        assert_eq!(frame.data().len(), 32);
        let bad = |size, stride, format, data: &[u8]| {
            assert_eq!(
                FrameView::new(size, stride, format, data).unwrap_err(),
                HalError::InvalidArgument
            );
        };
        bad(size, 15, PixelFormat::Xrgb8888, &data);
        bad(size, 16, PixelFormat::Xrgb8888, &data[..31]);
        bad(Size::new(0, 2), 16, PixelFormat::Xrgb8888, &data);
        bad(size, 16, PixelFormat::Nv12, &data);
        // A padded stride needs no padding after the last row.
        let padded = vec![0u8; 20 + 16];
        assert!(FrameView::new(size, 20, PixelFormat::Xrgb8888, &padded).is_ok());
    }

    #[test]
    fn mode_and_geometry() {
        let mode = DisplayMode::new(1920, 1080, 60);
        assert_eq!(mode.refresh_millihertz, 60_000);
        let info = DisplayInfo {
            id: DisplayId(1),
            connector: "HDMI-A-1".into(),
            internal: false,
            physical_mm: Size::new(527, 296),
            preferred_mode: mode,
        };
        let geometry = info.geometry(mode);
        assert_eq!(geometry.dpi, 93);
        assert_eq!((geometry.width_px, geometry.height_mm), (1920, 296));
    }
}
