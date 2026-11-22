//! Mock cameras and sensors.

use super::cell::MockCell;
use crate::camera::{Camera, CameraFacing, CameraFormat, CameraFrame, CameraInfo, CameraStream};
use crate::error::{HalError, HalResult};
use crate::sensors::{SensorInfo, SensorKind, SensorReading, Sensors};
use crate::types::{CameraId, SensorId};
use std::sync::{Arc, Mutex, PoisonError};

/// Microseconds between two frames or readings on the mock clock.
const SAMPLE_SPACING_US: u64 = 33_333;

#[derive(Debug)]
struct Device {
    info: CameraInfo,
    formats: Vec<CameraFormat>,
    torch: bool,
}

#[derive(Debug, Default)]
struct CameraState {
    devices: Vec<Device>,
    next_id: u32,
}

impl CameraState {
    fn device(&mut self, id: CameraId) -> HalResult<&mut Device> {
        self.devices
            .iter_mut()
            .find(|device| device.info.id == id)
            .ok_or(HalError::NotFound)
    }
}

/// Scriptable cameras. Starts without cameras.
#[derive(Debug, Default)]
pub struct MockCamera {
    cell: MockCell<CameraState>,
    streaming: Arc<Mutex<Vec<CameraId>>>,
}

impl MockCamera {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Adds a camera with the formats it offers.
    pub fn add_camera(
        &self,
        facing: CameraFacing,
        has_torch: bool,
        formats: &[CameraFormat],
    ) -> CameraId {
        let mut state = self.cell.state();
        let id = CameraId(state.next_id);
        state.next_id += 1;
        state.devices.push(Device {
            info: CameraInfo {
                id,
                facing,
                has_torch,
            },
            formats: formats.to_vec(),
            torch: false,
        });
        id
    }

    /// True if the torch of a camera is on.
    pub fn torch_on(&self, id: CameraId) -> bool {
        self.cell
            .state()
            .device(id)
            .map(|device| device.torch)
            .unwrap_or(false)
    }

    /// Cameras that currently have an open stream.
    pub fn streaming(&self) -> Vec<CameraId> {
        self.streaming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

struct MockCameraStream {
    id: CameraId,
    format: CameraFormat,
    sequence: u64,
    streaming: Arc<Mutex<Vec<CameraId>>>,
}

impl CameraStream for MockCameraStream {
    fn format(&self) -> CameraFormat {
        self.format
    }

    /// Delivers at once a frame whose bytes all carry the low byte of its sequence
    /// number; compressed formats get a fixed 64 byte payload.
    fn next_frame(&mut self, _timeout_ms: u32) -> HalResult<CameraFrame> {
        let length = self
            .format
            .format
            .frame_bytes(self.format.size)
            .unwrap_or(64);
        let length = usize::try_from(length).map_err(|_| HalError::InvalidArgument)?;
        let frame = CameraFrame {
            size: self.format.size,
            format: self.format.format,
            sequence: self.sequence,
            time_us: self.sequence * SAMPLE_SPACING_US,
            data: vec![(self.sequence % 256) as u8; length],
        };
        self.sequence += 1;
        Ok(frame)
    }
}

impl Drop for MockCameraStream {
    fn drop(&mut self) {
        self.streaming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|id| *id != self.id);
    }
}

impl Camera for MockCamera {
    fn list(&self) -> HalResult<Vec<CameraInfo>> {
        self.cell
            .call(|state| Ok(state.devices.iter().map(|device| device.info).collect()))
    }

    fn formats(&self, id: CameraId) -> HalResult<Vec<CameraFormat>> {
        self.cell
            .call(|state| Ok(state.device(id)?.formats.clone()))
    }

    fn open(&self, id: CameraId, format: CameraFormat) -> HalResult<Box<dyn CameraStream>> {
        self.cell.call(|state| {
            if !state.device(id)?.formats.contains(&format) {
                return Err(HalError::InvalidArgument);
            }
            let mut streaming = self
                .streaming
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if streaming.contains(&id) {
                return Err(HalError::Busy);
            }
            streaming.push(id);
            Ok(Box::new(MockCameraStream {
                id,
                format,
                sequence: 0,
                streaming: self.streaming.clone(),
            }) as Box<dyn CameraStream>)
        })
    }

    fn set_torch(&self, id: CameraId, on: bool) -> HalResult<()> {
        self.cell.call(|state| {
            let device = state.device(id)?;
            if !device.info.has_torch {
                return Err(HalError::NotSupported);
            }
            device.torch = on;
            Ok(())
        })
    }
}

#[derive(Debug)]
struct Sensor {
    info: SensorInfo,
    raw: [f32; 3],
    enabled: bool,
}

#[derive(Debug, Default)]
struct SensorState {
    sensors: Vec<Sensor>,
    next_id: u32,
    clock_us: u64,
}

impl SensorState {
    fn sensor(&mut self, id: SensorId) -> HalResult<&mut Sensor> {
        self.sensors
            .iter_mut()
            .find(|sensor| sensor.info.id == id)
            .ok_or(HalError::NotFound)
    }
}

/// Scriptable sensors. Starts without sensors.
#[derive(Debug, Default)]
pub struct MockSensors {
    cell: MockCell<SensorState>,
}

impl MockSensors {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Adds an enabled sensor with its current values.
    pub fn add_sensor(&self, kind: SensorKind, name: &str, raw: [f32; 3]) -> SensorId {
        let mut state = self.cell.state();
        let id = SensorId(state.next_id);
        state.next_id += 1;
        state.sensors.push(Sensor {
            info: SensorInfo {
                id,
                kind,
                name: name.to_string(),
            },
            raw,
            enabled: true,
        });
        id
    }

    /// Changes what a sensor measures.
    pub fn set_values(&self, id: SensorId, raw: [f32; 3]) -> HalResult<()> {
        self.cell.state().sensor(id)?.raw = raw;
        Ok(())
    }

    /// True if a sensor is switched on.
    pub fn is_enabled(&self, id: SensorId) -> bool {
        self.cell
            .state()
            .sensor(id)
            .map(|sensor| sensor.enabled)
            .unwrap_or(false)
    }
}

impl Sensors for MockSensors {
    fn list(&self, kind: Option<SensorKind>) -> HalResult<Vec<SensorInfo>> {
        self.cell.call(|state| {
            Ok(state
                .sensors
                .iter()
                .filter(|sensor| kind.is_none_or(|wanted| sensor.info.kind == wanted))
                .map(|sensor| sensor.info.clone())
                .collect())
        })
    }

    /// A sensor that is switched off refuses to measure.
    fn read(&self, id: SensorId) -> HalResult<SensorReading> {
        self.cell.call(|state| {
            state.clock_us += SAMPLE_SPACING_US;
            let time_us = state.clock_us;
            let sensor = state.sensor(id)?;
            if !sensor.enabled {
                return Err(HalError::Rejected);
            }
            Ok(SensorReading {
                sensor: id,
                kind: sensor.info.kind,
                time_us,
                raw: sensor.raw,
            })
        })
    }

    fn set_enabled(&self, id: SensorId, enabled: bool) -> HalResult<()> {
        self.cell.call(|state| {
            state.sensor(id)?.enabled = enabled;
            Ok(())
        })
    }
}
