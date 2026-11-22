//! Types shared by several device classes: geometry, pixel formats, levels and ids.

use crate::error::{HalError, HalResult};

/// Width and height in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Size {
    /// Creates a size.
    pub const fn new(width: u32, height: u32) -> Self {
        Size { width, height }
    }

    /// Number of pixels.
    pub const fn area(self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// True if either side is zero.
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Axis-aligned rectangle in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Horizontal position of the first column.
    pub x: i32,
    /// Vertical position of the first row.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Rect {
    /// Creates a rectangle.
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Rectangle covering a whole surface of the given size.
    pub const fn from_size(size: Size) -> Self {
        Rect::new(0, 0, size.width, size.height)
    }

    /// True if the rectangle covers no pixel.
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Column just past the last one.
    pub const fn right(&self) -> i64 {
        self.x as i64 + self.width as i64
    }

    /// Row just past the last one.
    pub const fn bottom(&self) -> i64 {
        self.y as i64 + self.height as i64
    }

    /// True if the point lies inside.
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && (x as i64) < self.right() && (y as i64) < self.bottom()
    }

    /// Common area of both rectangles, `None` if they do not overlap.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= i64::from(left) || bottom <= i64::from(top) {
            return None;
        }
        Some(Rect::new(
            left,
            top,
            (right - i64::from(left)) as u32,
            (bottom - i64::from(top)) as u32,
        ))
    }

    /// Smallest rectangle containing both. An empty rectangle does not contribute.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(
            left,
            top,
            (right - i64::from(left)).min(i64::from(u32::MAX)) as u32,
            (bottom - i64::from(top)).min(i64::from(u32::MAX)) as u32,
        )
    }

    /// Part of the rectangle that lies on a surface of the given size.
    pub fn clipped_to(&self, size: Size) -> Option<Rect> {
        self.intersection(&Rect::from_size(size))
    }
}

/// Layout of pixels or samples in a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// 32 bit, blue in the lowest byte, highest byte unused.
    Xrgb8888,
    /// 32 bit with alpha in the highest byte.
    Argb8888,
    /// 16 bit, 5-6-5.
    Rgb565,
    /// 24 bit, red first.
    Rgb888,
    /// Planar YUV 4:2:0 with interleaved chroma (camera, video).
    Nv12,
    /// Packed YUV 4:2:2 (camera).
    Yuyv,
    /// Motion JPEG frames (camera).
    Mjpeg,
}

impl PixelFormat {
    /// Bytes per pixel for packed formats with a fixed size, `None` otherwise.
    pub const fn bytes_per_pixel(self) -> Option<u32> {
        match self {
            PixelFormat::Xrgb8888 | PixelFormat::Argb8888 => Some(4),
            PixelFormat::Rgb888 => Some(3),
            PixelFormat::Rgb565 | PixelFormat::Yuyv => Some(2),
            PixelFormat::Nv12 | PixelFormat::Mjpeg => None,
        }
    }

    /// Smallest buffer that holds one frame, `None` for compressed formats.
    pub const fn frame_bytes(self, size: Size) -> Option<u64> {
        match self {
            PixelFormat::Nv12 => Some(size.area() * 3 / 2),
            PixelFormat::Mjpeg => None,
            other => match other.bytes_per_pixel() {
                Some(bytes) => Some(size.area() * bytes as u64),
                None => None,
            },
        }
    }
}

/// Rotation of a display against its natural orientation, clockwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Rotation {
    /// Natural orientation.
    #[default]
    Deg0,
    /// Quarter turn.
    Deg90,
    /// Upside down.
    Deg180,
    /// Three quarter turn.
    Deg270,
}

impl Rotation {
    /// Angle in degrees.
    pub const fn degrees(self) -> u16 {
        match self {
            Rotation::Deg0 => 0,
            Rotation::Deg90 => 90,
            Rotation::Deg180 => 180,
            Rotation::Deg270 => 270,
        }
    }

    /// Rotation for an angle; only multiples of 90 below 360 are valid.
    pub const fn from_degrees(degrees: u16) -> Option<Rotation> {
        match degrees {
            0 => Some(Rotation::Deg0),
            90 => Some(Rotation::Deg90),
            180 => Some(Rotation::Deg180),
            270 => Some(Rotation::Deg270),
            _ => None,
        }
    }

    /// True if width and height trade places.
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Deg90 | Rotation::Deg270)
    }

    /// Size the user sees for a panel of the given physical size.
    pub const fn logical_size(self, physical: Size) -> Size {
        if self.swaps_axes() {
            Size::new(physical.height, physical.width)
        } else {
            physical
        }
    }

    /// Maps a physical point (touch panel coordinates) to the logical, rotated surface.
    pub const fn physical_to_logical(self, x: i32, y: i32, physical: Size) -> (i32, i32) {
        let last_column = physical.width as i32 - 1;
        let last_row = physical.height as i32 - 1;
        match self {
            Rotation::Deg0 => (x, y),
            Rotation::Deg90 => (y, last_column - x),
            Rotation::Deg180 => (last_column - x, last_row - y),
            Rotation::Deg270 => (last_row - y, x),
        }
    }
}

/// A level between off and full (brightness, volume, vibration strength) in permille.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Level(u16);

impl Level {
    /// Lowest level.
    pub const MIN: Level = Level(0);
    /// Highest level.
    pub const MAX: Level = Level(1000);

    /// Creates a level from permille; values above 1000 are rejected.
    pub const fn from_permille(permille: u16) -> HalResult<Level> {
        if permille > 1000 {
            Err(HalError::InvalidArgument)
        } else {
            Ok(Level(permille))
        }
    }

    /// Creates a level from percent; values above 100 are rejected.
    pub const fn from_percent(percent: u8) -> HalResult<Level> {
        if percent > 100 {
            Err(HalError::InvalidArgument)
        } else {
            Ok(Level(percent as u16 * 10))
        }
    }

    /// Creates a level from a fraction; the value is clamped to 0..=1, NaN gives the minimum.
    pub fn from_fraction(fraction: f32) -> Level {
        if fraction.is_nan() {
            return Level::MIN;
        }
        Level((fraction.clamp(0.0, 1.0) * 1000.0).round() as u16)
    }

    /// Value in permille (0..=1000).
    pub const fn permille(self) -> u16 {
        self.0
    }

    /// Value in percent, rounded.
    pub const fn percent(self) -> u8 {
        ((self.0 + 5) / 10) as u8
    }

    /// Value as a fraction (0.0..=1.0).
    pub fn fraction(self) -> f32 {
        f32::from(self.0) / 1000.0
    }

    /// Scales to a device range `0..=max` (for example a backlight maximum), rounded.
    pub const fn scale_to(self, max: u32) -> u32 {
        ((self.0 as u64 * max as u64 + 500) / 1000) as u32
    }
}

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// Identifies a display for as long as it stays connected.
    DisplayId
);
id_type!(
    /// Identifies an input device for as long as it stays connected.
    InputDeviceId
);
id_type!(
    /// Identifies an audio output or input.
    EndpointId
);
id_type!(
    /// Identifies a camera.
    CameraId
);
id_type!(
    /// Identifies a sensor.
    SensorId
);
id_type!(
    /// Identifies an indicator light.
    LedId
);
id_type!(
    /// Identifies a block device.
    BlockDeviceId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let rect = Rect::new(10, 20, 30, 40);
        assert_eq!((rect.right(), rect.bottom()), (40, 60));
        assert!(rect.contains(10, 20) && rect.contains(39, 59));
        assert!(!rect.contains(40, 20) && !rect.contains(9, 20) && !rect.contains(10, 60));
        assert!(Rect::new(0, 0, 0, 5).is_empty() && !rect.is_empty());
        assert_eq!(Rect::from_size(Size::new(3, 4)), Rect::new(0, 0, 3, 4));
    }

    #[test]
    fn rect_intersection_and_union() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersection(&b), Some(Rect::new(5, 5, 5, 5)));
        assert_eq!(a.intersection(&Rect::new(10, 0, 5, 5)), None);
        assert_eq!(a.intersection(&Rect::new(3, 3, 0, 0)), None);
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert_eq!(a.union(&Rect::default()), a);
        assert_eq!(Rect::default().union(&b), b);
        let negative = Rect::new(-5, -5, 10, 10);
        assert_eq!(
            negative.clipped_to(Size::new(100, 100)),
            Some(Rect::new(0, 0, 5, 5))
        );
        assert_eq!(
            Rect::new(200, 0, 5, 5).clipped_to(Size::new(100, 100)),
            None
        );
    }

    #[test]
    fn rect_extremes_do_not_overflow() {
        let huge = Rect::new(i32::MAX, i32::MAX, u32::MAX, u32::MAX);
        assert!(huge.right() > i64::from(i32::MAX));
        assert_eq!(huge.clipped_to(Size::new(10, 10)), None);
        let wide = Rect::new(i32::MIN, 0, u32::MAX, 1).union(&Rect::new(i32::MAX, 0, 1, 1));
        assert_eq!(wide.width, u32::MAX);
    }

    #[test]
    fn pixel_formats() {
        let size = Size::new(4, 2);
        assert_eq!(PixelFormat::Xrgb8888.frame_bytes(size), Some(32));
        assert_eq!(PixelFormat::Rgb565.frame_bytes(size), Some(16));
        assert_eq!(PixelFormat::Rgb888.frame_bytes(size), Some(24));
        assert_eq!(PixelFormat::Nv12.frame_bytes(size), Some(12));
        assert_eq!(PixelFormat::Mjpeg.frame_bytes(size), None);
        assert_eq!(PixelFormat::Nv12.bytes_per_pixel(), None);
        assert_eq!(PixelFormat::Yuyv.bytes_per_pixel(), Some(2));
        assert!(Size::new(0, 9).is_empty());
    }

    #[test]
    fn rotation_mapping() {
        // Same results as the old shell's `phys_to_logical`.
        let panel = Size::new(1080, 2340);
        assert_eq!(Rotation::Deg0.physical_to_logical(5, 7, panel), (5, 7));
        assert_eq!(Rotation::Deg90.physical_to_logical(5, 7, panel), (7, 1074));
        assert_eq!(
            Rotation::Deg180.physical_to_logical(5, 7, panel),
            (1074, 2332)
        );
        assert_eq!(Rotation::Deg270.physical_to_logical(5, 7, panel), (2332, 5));
        assert_eq!(Rotation::Deg90.logical_size(panel), Size::new(2340, 1080));
        assert_eq!(Rotation::Deg180.logical_size(panel), panel);
        for degrees in [0, 90, 180, 270] {
            assert_eq!(
                Rotation::from_degrees(degrees).map(Rotation::degrees),
                Some(degrees)
            );
        }
        assert_eq!(Rotation::from_degrees(45), None);
        assert_eq!(Rotation::from_degrees(360), None);
    }

    #[test]
    fn levels() {
        assert_eq!(Level::from_permille(1000), Ok(Level::MAX));
        assert_eq!(Level::from_permille(1001), Err(HalError::InvalidArgument));
        assert_eq!(Level::from_percent(50).map(Level::permille), Ok(500));
        assert_eq!(Level::from_percent(101), Err(HalError::InvalidArgument));
        assert_eq!(Level::from_fraction(0.25).permille(), 250);
        assert_eq!(Level::from_fraction(7.0), Level::MAX);
        assert_eq!(Level::from_fraction(-1.0), Level::MIN);
        assert_eq!(Level::from_fraction(f32::NAN), Level::MIN);
        assert_eq!(Level::from_fraction(0.336).percent(), 34);
        assert!((Level::MAX.fraction() - 1.0).abs() < f32::EPSILON);
        assert_eq!(Level::MAX.scale_to(255), 255);
        assert_eq!(Level::MIN.scale_to(255), 0);
        assert_eq!(Level::from_permille(500).unwrap().scale_to(255), 128);
        assert_eq!(Level::MAX.scale_to(u32::MAX), u32::MAX);
    }

    #[test]
    fn ids_print_their_number() {
        assert_eq!(DisplayId(3).to_string(), "3");
        assert_eq!(CameraId::default(), CameraId(0));
        assert!(SensorId(1) < SensorId(2));
    }
}
