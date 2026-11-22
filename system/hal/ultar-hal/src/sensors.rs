//! Sensor and satellite positioning classes.

use crate::error::HalResult;
use crate::types::SensorId;

/// Kind of a sensor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SensorKind {
    /// Acceleration in m/s² on three axes.
    Accelerometer,
    /// Rotation rate in rad/s on three axes.
    Gyroscope,
    /// Magnetic field in µT on three axes.
    Magnetometer,
    /// Ambient light in lux.
    Light,
    /// Distance in centimetres (often only near or far).
    Proximity,
    /// Air pressure in hPa.
    Pressure,
    /// Steps counted since start.
    StepCounter,
    /// Temperature in degrees Celsius.
    Temperature,
    /// Opening angle of a hinge or fold in degrees.
    Hinge,
    /// Anything else the device offers.
    Other,
}

impl SensorKind {
    /// Number of values a reading of this kind carries.
    pub const fn axes(self) -> usize {
        match self {
            SensorKind::Accelerometer | SensorKind::Gyroscope | SensorKind::Magnetometer => 3,
            _ => 1,
        }
    }
}

/// Description of a sensor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SensorInfo {
    /// Id of the sensor.
    pub id: SensorId,
    /// Kind of sensor.
    pub kind: SensorKind,
    /// Name the sensor reports (technical, not for people).
    pub name: String,
}

/// One measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SensorReading {
    /// Sensor the reading comes from.
    pub sensor: SensorId,
    /// Kind of the sensor; decides how many values count.
    pub kind: SensorKind,
    /// Time of the measurement in microseconds on the monotonic clock.
    pub time_us: u64,
    /// The values in the unit of the kind; unused axes are zero.
    pub raw: [f32; 3],
}

impl SensorReading {
    /// The values that count for this kind of sensor.
    pub fn values(&self) -> &[f32] {
        &self.raw[..self.kind.axes()]
    }
}

/// Sensors of a backend.
pub trait Sensors: Send + Sync {
    /// Sensors of one kind, or every sensor with `None`.
    fn list(&self, kind: Option<SensorKind>) -> HalResult<Vec<SensorInfo>>;
    /// Takes one reading.
    fn read(&self, id: SensorId) -> HalResult<SensorReading>;
    /// Switches a sensor on or off to save energy.
    fn set_enabled(&self, id: SensorId, enabled: bool) -> HalResult<()>;
}

/// Quality of a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GnssFixKind {
    /// Position without height.
    TwoDimensional,
    /// Position with height.
    ThreeDimensional,
}

/// A position from the satellite receiver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GnssFix {
    /// Quality of the position.
    pub kind: GnssFixKind,
    /// Latitude in degrees, north positive.
    pub latitude_deg: f64,
    /// Longitude in degrees, east positive.
    pub longitude_deg: f64,
    /// Height above the ellipsoid in metres.
    pub altitude_m: f64,
    /// Horizontal accuracy in metres, if the receiver estimates it.
    pub accuracy_m: Option<f32>,
    /// Speed over ground in metres per second.
    pub speed_mps: f32,
    /// Course over ground in degrees from north.
    pub course_deg: f32,
    /// Number of satellites used for the position.
    pub satellites_used: u8,
    /// Time of the fix in milliseconds since the Unix epoch (UTC).
    pub unix_time_ms: u64,
}

impl GnssFix {
    /// True if the coordinates are inside the valid ranges.
    pub fn is_plausible(&self) -> bool {
        (-90.0..=90.0).contains(&self.latitude_deg)
            && (-180.0..=180.0).contains(&self.longitude_deg)
    }
}

/// A satellite in view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SatelliteInfo {
    /// Satellite number.
    pub id: u16,
    /// Elevation above the horizon in degrees.
    pub elevation_deg: i16,
    /// Azimuth in degrees from north.
    pub azimuth_deg: i16,
    /// Signal to noise ratio in dB-Hz; `None` if the satellite is not tracked.
    pub snr_dbhz: Option<u8>,
    /// True if the satellite is used for the position.
    pub used: bool,
}

/// Satellite receiver of a backend.
pub trait Gnss: Send + Sync {
    /// Switches the receiver on.
    fn start(&self) -> HalResult<()>;
    /// Switches the receiver off.
    fn stop(&self) -> HalResult<()>;
    /// True while the receiver runs.
    fn is_running(&self) -> HalResult<bool>;
    /// Latest position; `None` while there is no fix.
    fn fix(&self) -> HalResult<Option<GnssFix>>;
    /// Satellites in view.
    fn satellites(&self) -> HalResult<Vec<SatelliteInfo>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_values_follow_kind() {
        let mut reading = SensorReading {
            sensor: SensorId(1),
            kind: SensorKind::Accelerometer,
            time_us: 0,
            raw: [0.1, 9.8, 0.3],
        };
        assert_eq!(reading.values(), &[0.1, 9.8, 0.3]);
        reading.kind = SensorKind::Light;
        assert_eq!(reading.values(), &[0.1]);
        assert_eq!(SensorKind::Gyroscope.axes(), 3);
        assert_eq!(SensorKind::Magnetometer.axes(), 3);
        assert_eq!(SensorKind::Hinge.axes(), 1);
    }

    #[test]
    fn fix_plausibility() {
        let mut fix = GnssFix {
            kind: GnssFixKind::ThreeDimensional,
            latitude_deg: 35.93,
            longitude_deg: 36.63,
            altitude_m: 440.0,
            accuracy_m: Some(4.0),
            speed_mps: 0.0,
            course_deg: 0.0,
            satellites_used: 9,
            unix_time_ms: 1_795_000_000_000,
        };
        assert!(fix.is_plausible());
        fix.latitude_deg = 90.5;
        assert!(!fix.is_plausible());
        fix.latitude_deg = -90.0;
        fix.longitude_deg = 180.0;
        assert!(fix.is_plausible());
        fix.longitude_deg = f64::NAN;
        assert!(!fix.is_plausible());
    }
}
