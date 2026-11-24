//! Atomic display state for NVIDIA KMS backends.

use super::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh_millihz: u32,
    pub stride: u32,
}

/// Largest scanout the Turing display engine drives (16K); also bounds the framebuffer size.
pub const MAX_DIMENSION: u32 = 16_384;

impl Mode {
    pub fn validate(self) -> Result<()> {
        let min_stride = self.width.checked_mul(4).ok_or(Error::InvalidArgument)?;
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
            || self.refresh_millihz == 0
            || self.stride < min_stride
            || self.stride > min_stride.saturating_add(4096).max(MAX_DIMENSION * 4)
        {
            return Err(Error::InvalidArgument);
        }
        Ok(())
    }

    /// Bytes the framebuffer must hold for this mode (32 bpp).
    pub fn framebuffer_bytes(self) -> u64 {
        u64::from(self.stride) * u64::from(self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtomicState {
    pub connector: u32,
    pub crtc: u32,
    pub framebuffer: u64,
    pub mode: Mode,
}

impl AtomicState {
    pub fn validate(self) -> Result<()> {
        self.mode.validate()?;
        if self.framebuffer == 0 {
            return Err(Error::InvalidArgument);
        }
        Ok(())
    }
}

pub trait Backend {
    fn atomic_commit(&mut self, state: &AtomicState) -> Result<()>;
}

pub struct Device<B> {
    backend: B,
    active: Option<AtomicState>,
}

impl<B: Backend> Device<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            active: None,
        }
    }

    pub fn active(&self) -> Option<AtomicState> {
        self.active
    }

    pub fn commit(&mut self, state: AtomicState) -> Result<()> {
        state.validate()?;
        self.backend.atomic_commit(&state)?;
        self.active = Some(state);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock;

    impl Backend for Mock {
        fn atomic_commit(&mut self, _state: &AtomicState) -> Result<()> {
            Ok(())
        }
    }

    fn mode() -> Mode {
        Mode {
            width: 1920,
            height: 1080,
            refresh_millihz: 60_000,
            stride: 1920 * 4,
        }
    }

    #[test]
    fn rejects_overflowing_or_oversized_modes() {
        let huge = Mode {
            width: u32::MAX,
            height: 1,
            refresh_millihz: 1,
            stride: u32::MAX,
        };
        assert!(huge.validate().is_err());
        assert!(Mode {
            width: 16_385,
            ..mode()
        }
        .validate()
        .is_err());
        assert!(Mode {
            height: 0,
            ..mode()
        }
        .validate()
        .is_err());
        assert!(Mode {
            stride: 1920 * 4 - 1,
            ..mode()
        }
        .validate()
        .is_err());
        assert!(Mode {
            stride: u32::MAX,
            ..mode()
        }
        .validate()
        .is_err());
        assert!(Mode {
            stride: 2048 * 4,
            ..mode()
        }
        .validate()
        .is_ok());
        assert_eq!(mode().framebuffer_bytes(), 1920 * 4 * 1080);
    }

    #[test]
    fn commits_only_valid_state() {
        let mut device = Device::new(Mock);
        assert!(device
            .commit(AtomicState {
                connector: 1,
                crtc: 1,
                framebuffer: 0x1000,
                mode: mode(),
            })
            .is_ok());
        assert!(device.active().is_some());
        assert!(device
            .commit(AtomicState {
                connector: 1,
                crtc: 1,
                framebuffer: 0,
                mode: mode(),
            })
            .is_err());
    }
}
