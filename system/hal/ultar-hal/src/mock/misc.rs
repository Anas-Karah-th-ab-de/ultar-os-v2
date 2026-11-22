//! Mock vibration motor, indicator lights, block storage and TPM.

use super::cell::MockCell;
use crate::error::{HalError, HalResult};
use crate::misc::{
    BlockDeviceInfo, Haptics, LedInfo, LedState, Leds, Storage, Tpm, TpmVersion, TPM_DIGEST_LEN,
};
use crate::types::{BlockDeviceId, LedId, Level};

#[derive(Debug, Default)]
struct Motor {
    pulses: Vec<(u32, Level)>,
    cancels: usize,
}

/// Scriptable vibration motor that records what it was asked to do.
#[derive(Debug, Default)]
pub struct MockHaptics {
    cell: MockCell<Motor>,
}

impl MockHaptics {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Vibrations requested so far as (duration in milliseconds, strength).
    pub fn pulses(&self) -> Vec<(u32, Level)> {
        self.cell.state().pulses.clone()
    }

    /// Number of cancel requests so far.
    pub fn cancels(&self) -> usize {
        self.cell.state().cancels
    }
}

impl Haptics for MockHaptics {
    fn vibrate(&self, duration_ms: u32, strength: Level) -> HalResult<()> {
        self.cell.call(|motor| {
            if duration_ms == 0 {
                return Err(HalError::InvalidArgument);
            }
            motor.pulses.push((duration_ms, strength));
            Ok(())
        })
    }

    fn cancel(&self) -> HalResult<()> {
        self.cell.call(|motor| {
            motor.cancels += 1;
            Ok(())
        })
    }
}

/// Scriptable indicator lights. Starts without lights.
#[derive(Debug, Default)]
pub struct MockLeds {
    cell: MockCell<Vec<(LedInfo, LedState)>>,
}

impl MockLeds {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Adds a light that is switched off.
    pub fn add_led(&self, name: &str, has_color: bool) -> LedId {
        let mut lights = self.cell.state();
        let id = LedId(lights.len() as u32);
        lights.push((
            LedInfo {
                id,
                name: name.to_string(),
                has_color,
            },
            LedState::OFF,
        ));
        id
    }
}

impl Leds for MockLeds {
    fn list(&self) -> HalResult<Vec<LedInfo>> {
        self.cell
            .call(|lights| Ok(lights.iter().map(|(info, _)| info.clone()).collect()))
    }

    fn state(&self, id: LedId) -> HalResult<LedState> {
        self.cell.call(|lights| {
            lights
                .iter()
                .find(|(info, _)| info.id == id)
                .map(|(_, state)| *state)
                .ok_or(HalError::NotFound)
        })
    }

    fn set(&self, id: LedId, state: LedState) -> HalResult<()> {
        self.cell.call(|lights| {
            let light = lights
                .iter_mut()
                .find(|(info, _)| info.id == id)
                .ok_or(HalError::NotFound)?;
            light.1 = state;
            Ok(())
        })
    }
}

#[derive(Debug)]
struct Disk {
    info: BlockDeviceInfo,
    data: Vec<u8>,
    flushes: usize,
}

impl Disk {
    /// Byte range of a block run, checked against block size and capacity.
    fn range(&self, first_block: u64, length: usize) -> HalResult<std::ops::Range<usize>> {
        let block_size = self.info.block_size as usize;
        if !length.is_multiple_of(block_size) {
            return Err(HalError::InvalidArgument);
        }
        let start = usize::try_from(first_block)
            .ok()
            .and_then(|block| block.checked_mul(block_size))
            .ok_or(HalError::InvalidArgument)?;
        let end = start.checked_add(length).ok_or(HalError::InvalidArgument)?;
        if end > self.data.len() {
            return Err(HalError::InvalidArgument);
        }
        Ok(start..end)
    }
}

/// Scriptable block storage held in memory. Starts without devices.
#[derive(Debug, Default)]
pub struct MockStorage {
    cell: MockCell<Vec<Disk>>,
}

impl MockStorage {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Adds a zero-filled device. A block size of zero is taken as 512.
    pub fn add_device(
        &self,
        name: &str,
        block_size: u32,
        block_count: u32,
        read_only: bool,
    ) -> BlockDeviceId {
        let mut disks = self.cell.state();
        let id = BlockDeviceId(disks.len() as u32);
        let block_size = if block_size == 0 { 512 } else { block_size };
        disks.push(Disk {
            info: BlockDeviceInfo {
                id,
                name: name.to_string(),
                block_size,
                block_count: u64::from(block_count),
                removable: false,
                read_only,
            },
            data: vec![0; block_size as usize * block_count as usize],
            flushes: 0,
        });
        id
    }

    /// Number of flushes a device received.
    pub fn flushes(&self, id: BlockDeviceId) -> usize {
        self.cell
            .state()
            .iter()
            .find(|disk| disk.info.id == id)
            .map(|disk| disk.flushes)
            .unwrap_or(0)
    }
}

fn disk(disks: &mut [Disk], id: BlockDeviceId) -> HalResult<&mut Disk> {
    disks
        .iter_mut()
        .find(|disk| disk.info.id == id)
        .ok_or(HalError::NotFound)
}

impl Storage for MockStorage {
    fn devices(&self) -> HalResult<Vec<BlockDeviceInfo>> {
        self.cell
            .call(|disks| Ok(disks.iter().map(|disk| disk.info.clone()).collect()))
    }

    fn read_blocks(&self, id: BlockDeviceId, first_block: u64, buffer: &mut [u8]) -> HalResult<()> {
        self.cell.call(|disks| {
            let disk = disk(disks, id)?;
            let range = disk.range(first_block, buffer.len())?;
            buffer.copy_from_slice(&disk.data[range]);
            Ok(())
        })
    }

    fn write_blocks(&self, id: BlockDeviceId, first_block: u64, data: &[u8]) -> HalResult<()> {
        self.cell.call(|disks| {
            let disk = disk(disks, id)?;
            if disk.info.read_only {
                return Err(HalError::PermissionDenied);
            }
            let range = disk.range(first_block, data.len())?;
            disk.data[range].copy_from_slice(data);
            Ok(())
        })
    }

    fn flush(&self, id: BlockDeviceId) -> HalResult<()> {
        self.cell.call(|disks| {
            disk(disks, id)?.flushes += 1;
            Ok(())
        })
    }
}

/// Number of platform configuration registers of the mock TPM.
const PCR_COUNT: usize = 24;
/// Marker at the start of a blob sealed by the mock TPM.
const SEAL_MAGIC: &[u8; 4] = b"MTPM";

#[derive(Debug)]
struct Chip {
    pcrs: [[u8; TPM_DIGEST_LEN]; PCR_COUNT],
    random_state: u64,
}

impl Chip {
    fn next_random(&mut self) -> u8 {
        // Fixed linear congruential generator: deterministic "random" bytes.
        self.random_state = self
            .random_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.random_state >> 56) as u8
    }

    /// Mixes the registers named in the mask into one binding value.
    fn binding(&self, pcr_mask: u32) -> [u8; TPM_DIGEST_LEN] {
        let mut binding = [0x5a_u8; TPM_DIGEST_LEN];
        for (index, pcr) in self.pcrs.iter().enumerate() {
            if pcr_mask & (1 << index) != 0 {
                mix(&mut binding, pcr);
            }
        }
        binding
    }
}

/// Folds `input` into `target`. Not a cryptographic hash: the mock only needs a
/// value that changes whenever a register changes.
fn mix(target: &mut [u8; TPM_DIGEST_LEN], input: &[u8; TPM_DIGEST_LEN]) {
    let mut carry = 0x9e_u8;
    for (position, byte) in target.iter_mut().enumerate() {
        carry = carry
            .rotate_left(3)
            .wrapping_add(input[position])
            .wrapping_add(*byte)
            .wrapping_mul(31)
            ^ (position as u8);
        *byte = carry;
    }
}

/// Scriptable TPM 2.0. Deterministic and **not** secure: sealing only binds a blob
/// to the register values so that the logic above it can be tested.
#[derive(Debug)]
pub struct MockTpm {
    cell: MockCell<Chip>,
}

impl Default for MockTpm {
    fn default() -> Self {
        MockTpm {
            cell: MockCell::new(Chip {
                pcrs: [[0; TPM_DIGEST_LEN]; PCR_COUNT],
                random_state: 0x756c_7461_725f_6f73,
            }),
        }
    }
}

impl MockTpm {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }
}

impl Tpm for MockTpm {
    fn version(&self) -> HalResult<TpmVersion> {
        self.cell.call(|_| Ok(TpmVersion::V2_0))
    }

    fn random(&self, buffer: &mut [u8]) -> HalResult<()> {
        self.cell.call(|chip| {
            for byte in buffer.iter_mut() {
                *byte = chip.next_random();
            }
            Ok(())
        })
    }

    fn pcr_read(&self, index: u8) -> HalResult<[u8; TPM_DIGEST_LEN]> {
        self.cell.call(|chip| {
            chip.pcrs
                .get(usize::from(index))
                .copied()
                .ok_or(HalError::InvalidArgument)
        })
    }

    fn pcr_extend(&self, index: u8, digest: &[u8; TPM_DIGEST_LEN]) -> HalResult<()> {
        self.cell.call(|chip| {
            let pcr = chip
                .pcrs
                .get_mut(usize::from(index))
                .ok_or(HalError::InvalidArgument)?;
            mix(pcr, digest);
            Ok(())
        })
    }

    fn seal(&self, secret: &[u8], pcr_mask: u32) -> HalResult<Vec<u8>> {
        self.cell.call(|chip| {
            if pcr_mask >> PCR_COUNT != 0 {
                return Err(HalError::InvalidArgument);
            }
            let binding = chip.binding(pcr_mask);
            let mut sealed = Vec::with_capacity(secret.len() + 8 + TPM_DIGEST_LEN);
            sealed.extend_from_slice(SEAL_MAGIC);
            sealed.extend_from_slice(&pcr_mask.to_le_bytes());
            sealed.extend_from_slice(&binding);
            sealed.extend(
                secret
                    .iter()
                    .enumerate()
                    .map(|(position, byte)| byte ^ binding[position % TPM_DIGEST_LEN]),
            );
            Ok(sealed)
        })
    }

    fn unseal(&self, sealed: &[u8]) -> HalResult<Vec<u8>> {
        self.cell.call(|chip| {
            let header = 8 + TPM_DIGEST_LEN;
            if sealed.len() < header || &sealed[..4] != SEAL_MAGIC {
                return Err(HalError::Rejected);
            }
            let mask_bytes: [u8; 4] = sealed[4..8].try_into().map_err(|_| HalError::Rejected)?;
            let binding = chip.binding(u32::from_le_bytes(mask_bytes));
            if sealed[8..header] != binding {
                return Err(HalError::Rejected);
            }
            Ok(sealed[header..]
                .iter()
                .enumerate()
                .map(|(position, byte)| byte ^ binding[position % TPM_DIGEST_LEN])
                .collect())
        })
    }
}
