//! Input class: touch, keys, pointers, scrolling and device hotplug.

use crate::error::HalResult;
use crate::types::InputDeviceId;

/// Kind of an input device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputDeviceKind {
    /// Touch screen.
    Touchscreen,
    /// Keyboard with letter keys.
    Keyboard,
    /// Mouse or other relative pointer.
    Mouse,
    /// Touchpad.
    Touchpad,
    /// Hardware buttons of the device (power, volume).
    Buttons,
    /// Stylus or drawing tablet.
    Stylus,
}

impl InputDeviceKind {
    /// True if the device moves a pointer.
    pub const fn is_pointer(self) -> bool {
        matches!(self, InputDeviceKind::Mouse | InputDeviceKind::Touchpad)
    }
}

/// Description of an input device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDeviceInfo {
    /// Id valid while the device stays connected.
    pub id: InputDeviceId,
    /// Kind of device.
    pub kind: InputDeviceKind,
    /// Name the device reports (technical, not for people).
    pub name: String,
    /// True if the device is built in, false if it was plugged in or paired.
    pub internal: bool,
}

/// State of a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyState {
    /// The key went up.
    Released,
    /// The key went down.
    Pressed,
    /// The key is held and repeats.
    Repeated,
}

/// Button of a pointer device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// Primary button.
    Primary,
    /// Secondary button.
    Secondary,
    /// Middle button or wheel click.
    Middle,
    /// Any further button, by its number.
    Other(u16),
}

/// What happened on an input device.
///
/// Touch and absolute pointer positions are in physical device coordinates scaled to
/// 0..=65535 on each axis, so they are independent of resolution and rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEventKind {
    /// A finger touched the screen.
    TouchDown {
        /// Finger slot, stable until the finger lifts.
        slot: u8,
        /// Horizontal position.
        x: u16,
        /// Vertical position.
        y: u16,
    },
    /// A finger moved.
    TouchMove {
        /// Finger slot.
        slot: u8,
        /// Horizontal position.
        x: u16,
        /// Vertical position.
        y: u16,
    },
    /// A finger lifted.
    TouchUp {
        /// Finger slot.
        slot: u8,
    },
    /// The touch sequence was taken away (palm, system gesture); every slot is released.
    TouchCancel,
    /// A key changed. `code` is the hardware key code (Linux input numbering).
    Key {
        /// Hardware key code.
        code: u16,
        /// New state.
        state: KeyState,
    },
    /// The pointer moved by a distance.
    PointerMotion {
        /// Horizontal distance in device units.
        dx: f32,
        /// Vertical distance in device units.
        dy: f32,
    },
    /// The pointer jumped to a position (tablet, virtual machine).
    PointerAbsolute {
        /// Horizontal position.
        x: u16,
        /// Vertical position.
        y: u16,
    },
    /// A pointer button changed.
    PointerButton {
        /// The button.
        button: PointerButton,
        /// True if it went down.
        pressed: bool,
    },
    /// Scrolling in wheel steps; positive `dy` scrolls up.
    Scroll {
        /// Horizontal steps.
        dx: f32,
        /// Vertical steps.
        dy: f32,
    },
    /// The device was connected; details through [`Input::devices`].
    DeviceAdded,
    /// The device was removed.
    DeviceRemoved,
}

/// One input event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputEvent {
    /// Device the event comes from.
    pub device: InputDeviceId,
    /// Time of the event in microseconds on the monotonic clock.
    pub time_us: u64,
    /// What happened.
    pub kind: InputEventKind,
}

/// Input devices of a backend.
pub trait Input: Send + Sync {
    /// Connected input devices.
    fn devices(&self) -> HalResult<Vec<InputDeviceInfo>>;
    /// Waits up to `timeout_ms` for events and returns them in order of arrival.
    /// Returns an empty list on timeout; zero means "do not wait".
    fn poll(&self, timeout_ms: u32) -> HalResult<Vec<InputEvent>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_kinds() {
        assert!(InputDeviceKind::Mouse.is_pointer());
        assert!(InputDeviceKind::Touchpad.is_pointer());
        assert!(!InputDeviceKind::Touchscreen.is_pointer());
        assert!(!InputDeviceKind::Keyboard.is_pointer());
        assert!(!InputDeviceKind::Buttons.is_pointer());
        assert!(!InputDeviceKind::Stylus.is_pointer());
    }
}
