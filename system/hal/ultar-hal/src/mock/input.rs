//! Mock input devices with an injectable event queue.

use super::cell::MockCell;
use crate::error::{HalError, HalResult};
use crate::input::{Input, InputDeviceInfo, InputDeviceKind, InputEvent, InputEventKind, KeyState};
use crate::types::InputDeviceId;
use std::collections::VecDeque;

/// Microseconds the mock clock advances per injected event.
const EVENT_SPACING_US: u64 = 1_000;

#[derive(Debug, Default)]
struct InputState {
    devices: Vec<InputDeviceInfo>,
    queue: VecDeque<InputEvent>,
    next_id: u32,
    clock_us: u64,
}

impl InputState {
    fn push(&mut self, device: InputDeviceId, kind: InputEventKind) {
        self.clock_us += EVENT_SPACING_US;
        self.queue.push_back(InputEvent {
            device,
            time_us: self.clock_us,
            kind,
        });
    }
}

/// Scriptable input devices. Starts without any device; time stamps are deterministic
/// (one millisecond per injected event).
#[derive(Debug, Default)]
pub struct MockInput {
    cell: MockCell<InputState>,
}

impl MockInput {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Connects a device and queues the hotplug event.
    pub fn add_device(&self, kind: InputDeviceKind, name: &str, internal: bool) -> InputDeviceId {
        let mut state = self.cell.state();
        let id = InputDeviceId(state.next_id);
        state.next_id += 1;
        state.devices.push(InputDeviceInfo {
            id,
            kind,
            name: name.to_string(),
            internal,
        });
        state.push(id, InputEventKind::DeviceAdded);
        id
    }

    /// Removes a device and queues the hotplug event.
    pub fn remove_device(&self, id: InputDeviceId) -> HalResult<()> {
        let mut state = self.cell.state();
        let before = state.devices.len();
        state.devices.retain(|device| device.id != id);
        if state.devices.len() == before {
            return Err(HalError::NotFound);
        }
        state.push(id, InputEventKind::DeviceRemoved);
        Ok(())
    }

    /// Queues one event from a connected device.
    pub fn inject(&self, device: InputDeviceId, kind: InputEventKind) -> HalResult<()> {
        let mut state = self.cell.state();
        if !state.devices.iter().any(|info| info.id == device) {
            return Err(HalError::NotFound);
        }
        state.push(device, kind);
        Ok(())
    }

    /// Queues a one-finger tap (down and up) at a position.
    pub fn inject_tap(&self, device: InputDeviceId, x: u16, y: u16) -> HalResult<()> {
        self.inject(device, InputEventKind::TouchDown { slot: 0, x, y })?;
        self.inject(device, InputEventKind::TouchUp { slot: 0 })
    }

    /// Queues a key press followed by its release.
    pub fn inject_key(&self, device: InputDeviceId, code: u16) -> HalResult<()> {
        for state in [KeyState::Pressed, KeyState::Released] {
            self.inject(device, InputEventKind::Key { code, state })?;
        }
        Ok(())
    }

    /// Number of events waiting to be polled.
    pub fn pending(&self) -> usize {
        self.cell.state().queue.len()
    }
}

impl Input for MockInput {
    fn devices(&self) -> HalResult<Vec<InputDeviceInfo>> {
        self.cell.call(|state| Ok(state.devices.clone()))
    }

    /// Returns everything queued at once and never waits.
    fn poll(&self, _timeout_ms: u32) -> HalResult<Vec<InputEvent>> {
        self.cell.call(|state| Ok(state.queue.drain(..).collect()))
    }
}
