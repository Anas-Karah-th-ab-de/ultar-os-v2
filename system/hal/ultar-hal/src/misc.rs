//! Smaller classes: haptics, indicator lights, block storage and the TPM.

use crate::error::HalResult;
use crate::types::{BlockDeviceId, LedId, Level};

/// Vibration motor of a backend.
pub trait Haptics: Send + Sync {
    /// Vibrates for `duration_ms` with the given strength.
    fn vibrate(&self, duration_ms: u32, strength: Level) -> HalResult<()>;
    /// Stops a running vibration.
    fn cancel(&self) -> HalResult<()>;
}

/// Description of an indicator light.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedInfo {
    /// Id of the light.
    pub id: LedId,
    /// Name the platform gives the light (technical, not for people).
    pub name: String,
    /// True if the colour can be chosen; otherwise only on and off.
    pub has_color: bool,
}

/// What an indicator light shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LedState {
    /// Red part.
    pub red: u8,
    /// Green part.
    pub green: u8,
    /// Blue part.
    pub blue: u8,
    /// Time switched on per blink in milliseconds; zero together with `off_ms` = steady.
    pub on_ms: u32,
    /// Time switched off per blink in milliseconds.
    pub off_ms: u32,
}

impl LedState {
    /// Light switched off.
    pub const OFF: LedState = LedState {
        red: 0,
        green: 0,
        blue: 0,
        on_ms: 0,
        off_ms: 0,
    };

    /// Steady light in one colour.
    pub const fn solid(red: u8, green: u8, blue: u8) -> Self {
        LedState {
            red,
            green,
            blue,
            on_ms: 0,
            off_ms: 0,
        }
    }

    /// Blinking light in one colour.
    pub const fn blinking(red: u8, green: u8, blue: u8, on_ms: u32, off_ms: u32) -> Self {
        LedState {
            red,
            green,
            blue,
            on_ms,
            off_ms,
        }
    }

    /// True if the light is dark.
    pub const fn is_off(&self) -> bool {
        self.red == 0 && self.green == 0 && self.blue == 0
    }

    /// True if the light blinks.
    pub const fn is_blinking(&self) -> bool {
        !self.is_off() && self.on_ms > 0 && self.off_ms > 0
    }
}

/// Indicator lights of a backend.
pub trait Leds: Send + Sync {
    /// Every light.
    fn list(&self) -> HalResult<Vec<LedInfo>>;
    /// Current state of a light.
    fn state(&self, id: LedId) -> HalResult<LedState>;
    /// Sets a light.
    fn set(&self, id: LedId, state: LedState) -> HalResult<()>;
}

/// Description of a block device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockDeviceInfo {
    /// Id of the device.
    pub id: BlockDeviceId,
    /// Kernel name of the device (technical, not for people).
    pub name: String,
    /// Size of one block in bytes.
    pub block_size: u32,
    /// Number of blocks.
    pub block_count: u64,
    /// True for media the user can take out.
    pub removable: bool,
    /// True if the device cannot be written.
    pub read_only: bool,
}

impl BlockDeviceInfo {
    /// Capacity in bytes.
    pub const fn capacity_bytes(&self) -> u64 {
        self.block_size as u64 * self.block_count
    }
}

/// Block storage of a backend.
pub trait Storage: Send + Sync {
    /// Every block device.
    fn devices(&self) -> HalResult<Vec<BlockDeviceInfo>>;
    /// Reads whole blocks starting at `first_block`; the buffer length must be a
    /// multiple of the block size.
    fn read_blocks(&self, id: BlockDeviceId, first_block: u64, buffer: &mut [u8]) -> HalResult<()>;
    /// Writes whole blocks starting at `first_block`; the data length must be a
    /// multiple of the block size.
    fn write_blocks(&self, id: BlockDeviceId, first_block: u64, data: &[u8]) -> HalResult<()>;
    /// Makes sure everything written has reached the medium.
    fn flush(&self, id: BlockDeviceId) -> HalResult<()>;
}

/// Length of a TPM digest in bytes (SHA-256 bank).
pub const TPM_DIGEST_LEN: usize = 32;

/// Version of the TPM specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TpmVersion {
    /// TPM 1.2.
    V1_2,
    /// TPM 2.0.
    V2_0,
}

/// Trusted platform module of a backend.
pub trait Tpm: Send + Sync {
    /// Version of the module.
    fn version(&self) -> HalResult<TpmVersion>;
    /// Fills the buffer with random bytes from the module.
    fn random(&self, buffer: &mut [u8]) -> HalResult<()>;
    /// Reads a platform configuration register.
    fn pcr_read(&self, index: u8) -> HalResult<[u8; TPM_DIGEST_LEN]>;
    /// Extends a platform configuration register with a digest.
    fn pcr_extend(&self, index: u8, digest: &[u8; TPM_DIGEST_LEN]) -> HalResult<()>;
    /// Seals a secret to the current value of the registers named in `pcr_mask`
    /// (bit n = register n) and returns the sealed blob.
    fn seal(&self, secret: &[u8], pcr_mask: u32) -> HalResult<Vec<u8>>;
    /// Opens a sealed blob; fails with [`crate::HalError::Rejected`] if the registers
    /// changed or the blob is not from this module.
    fn unseal(&self, sealed: &[u8]) -> HalResult<Vec<u8>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn led_states() {
        assert!(LedState::OFF.is_off() && !LedState::OFF.is_blinking());
        assert_eq!(LedState::default(), LedState::OFF);
        let red = LedState::solid(255, 0, 0);
        assert!(!red.is_off() && !red.is_blinking());
        assert!(LedState::blinking(0, 0, 9, 500, 500).is_blinking());
        assert!(!LedState::blinking(0, 0, 0, 500, 500).is_blinking());
        assert!(!LedState::blinking(0, 0, 9, 500, 0).is_blinking());
    }

    #[test]
    fn capacity() {
        let disk = BlockDeviceInfo {
            id: BlockDeviceId(0),
            name: "disk0".into(),
            block_size: 4096,
            block_count: 1 << 28,
            removable: false,
            read_only: false,
        };
        assert_eq!(disk.capacity_bytes(), 1 << 40);
    }
}
