//! The full capability description of a device: flags plus a few numbers.

use crate::capability::{Capability, CapabilitySet};
use serde::{Deserialize, Serialize};

/// Widest SIMD instruction set usable on every core.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimdLevel {
    /// No SIMD known.
    #[default]
    None,
    /// x86 SSE2.
    Sse2,
    /// x86 AVX2.
    Avx2,
    /// x86 AVX-512.
    Avx512,
    /// ARM NEON.
    Neon,
    /// ARM SVE.
    Sve,
}

/// Summary of the CPU topology (enough for scheduling and model selection decisions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuTopology {
    /// Number of performance ("big") cores.
    #[serde(default)]
    pub performance_cores: u16,
    /// Number of efficiency ("little") cores.
    #[serde(default)]
    pub efficiency_cores: u16,
    /// Widest SIMD level.
    #[serde(default)]
    pub simd: SimdLevel,
}

impl CpuTopology {
    /// Total number of cores.
    pub fn total_cores(&self) -> u32 {
        u32::from(self.performance_cores) + u32::from(self.efficiency_cores)
    }

    /// True if the CPU mixes performance and efficiency cores.
    pub fn is_heterogeneous(&self) -> bool {
        self.performance_cores > 0 && self.efficiency_cores > 0
    }
}

/// Size and density of the primary display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayGeometry {
    /// Width in pixels.
    pub width_px: u32,
    /// Height in pixels.
    pub height_px: u32,
    /// Physical width in millimetres (0 = unknown).
    #[serde(default)]
    pub width_mm: u32,
    /// Physical height in millimetres (0 = unknown).
    #[serde(default)]
    pub height_mm: u32,
    /// Pixel density in dots per inch (0 = unknown).
    #[serde(default)]
    pub dpi: u16,
}

impl DisplayGeometry {
    /// Density assumed when the physical size is unknown.
    pub const FALLBACK_DPI: u16 = 96;

    /// Builds the geometry from pixel and millimetre sizes and computes the density.
    pub fn from_physical(width_px: u32, height_px: u32, width_mm: u32, height_mm: u32) -> Self {
        let dpi = if width_mm == 0 {
            0
        } else {
            let exact = (f64::from(width_px) * 25.4 / f64::from(width_mm)).round();
            exact.clamp(0.0, f64::from(u16::MAX)) as u16
        };
        DisplayGeometry {
            width_px,
            height_px,
            width_mm,
            height_mm,
            dpi,
        }
    }

    /// Density to use for scaling: the known value or [`Self::FALLBACK_DPI`].
    pub fn effective_dpi(&self) -> u16 {
        if self.dpi == 0 {
            Self::FALLBACK_DPI
        } else {
            self.dpi
        }
    }

    /// Physical diagonal in millimetres, if the physical size is known.
    pub fn diagonal_mm(&self) -> Option<u32> {
        if self.width_mm == 0 || self.height_mm == 0 {
            return None;
        }
        let diagonal = f64::from(self.width_mm).hypot(f64::from(self.height_mm));
        Some(diagonal.round() as u32)
    }
}

/// Everything the system knows about what the device can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Present capabilities.
    #[serde(default)]
    pub flags: CapabilitySet,
    /// Number of connected displays (built-in and external).
    #[serde(default)]
    pub display_count: u8,
    /// Installed memory in mebibytes.
    #[serde(default)]
    pub memory_mb: u32,
    /// CPU topology summary.
    #[serde(default)]
    pub cpu: CpuTopology,
    /// Geometry of the primary display, if there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_display: Option<DisplayGeometry>,
}

impl Capabilities {
    /// True if the capability is present.
    pub fn has(&self, capability: Capability) -> bool {
        self.flags.contains(capability)
    }

    /// Returns a copy with `capability` added (builder style, handy in tests).
    #[must_use]
    pub fn with(mut self, capability: Capability) -> Self {
        self.flags.insert(capability);
        self
    }

    /// Returns a copy with the display count set and the external-display flag kept unchanged.
    #[must_use]
    pub fn with_displays(mut self, count: u8) -> Self {
        self.display_count = count;
        self
    }

    /// True if at least one display is connected.
    pub fn has_display(&self) -> bool {
        self.display_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_summary() {
        let cpu = CpuTopology {
            performance_cores: 2,
            efficiency_cores: 6,
            simd: SimdLevel::Neon,
        };
        assert_eq!(cpu.total_cores(), 8);
        assert!(cpu.is_heterogeneous());
        assert!(!CpuTopology::default().is_heterogeneous());
        assert_eq!(CpuTopology::default().total_cores(), 0);
    }

    #[test]
    fn display_density() {
        // Samsung A30: 1080 x 2340 on 68 x 147 mm.
        let phone = DisplayGeometry::from_physical(1080, 2340, 68, 147);
        assert_eq!(phone.dpi, 403);
        assert_eq!(phone.effective_dpi(), 403);
        assert_eq!(phone.diagonal_mm(), Some(162));
        let unknown = DisplayGeometry::from_physical(1920, 1080, 0, 0);
        assert_eq!(unknown.dpi, 0);
        assert_eq!(unknown.effective_dpi(), DisplayGeometry::FALLBACK_DPI);
        assert_eq!(unknown.diagonal_mm(), None);
    }

    #[test]
    fn builder_helpers() {
        let caps = Capabilities::default()
            .with(Capability::Touch)
            .with_displays(2);
        assert!(caps.has(Capability::Touch));
        assert!(!caps.has(Capability::Modem));
        assert!(caps.has_display());
        assert!(!Capabilities::default().has_display());
    }

    #[test]
    fn missing_fields_default() {
        let caps: Capabilities = toml::from_str("").unwrap();
        assert_eq!(caps, Capabilities::default());
        let cpu: CpuTopology = toml::from_str("simd = \"avx2\"").unwrap();
        assert_eq!(cpu.simd, SimdLevel::Avx2);
        assert!(toml::from_str::<CpuTopology>("simd = \"mmx\"").is_err());
    }
}
