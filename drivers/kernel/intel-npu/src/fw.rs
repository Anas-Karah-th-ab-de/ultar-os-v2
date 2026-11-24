//! The container of the NPU firmware: a 4 KiB header, a 4 KiB version block, then the image.
//! The header says where the image runs in the NPU's address space, where it starts and how much
//! run-time memory it wants. Fields and checks as the Linux `ivpu` driver describes them
//! (`vpu_boot_api.h`: `struct vpu_firmware_header`, 4-byte packing; `ivpu_fw.c`).

use crate::ident::{Model, Range};
use crate::{Error, Result};
use alloc::string::String;

/// Size of the header block.
pub const HEADER_SIZE: usize = 4096;
/// Size of the version block after the header.
pub const VERSION_BLOCK_SIZE: usize = 4096;
/// Where the image starts in the file.
pub const IMAGE_OFFSET: usize = HEADER_SIZE + VERSION_BLOCK_SIZE;
/// The header version this driver reads.
pub const HEADER_VERSION: u32 = 1;
/// Largest accepted image of the SHAVE inference processors.
const SHAVE_NN_MAX_SIZE: u32 = 2 << 20;
/// Oldest boot and job interface (major) this driver's plan covers.
const MIN_API_MAJOR: u16 = 3;
/// Index of the boot interface in the header's version table.
const API_BOOT: usize = 0;
/// Index of the job (JSM) interface in the header's version table.
const API_JSM: usize = 4;

// byte offsets of the header fields (4-byte packing)
const OFF_HEADER_VERSION: usize = 0;
const OFF_IMAGE_LOAD_ADDRESS: usize = 8;
const OFF_IMAGE_SIZE: usize = 16;
const OFF_ENTRY_POINT: usize = 20;
const OFF_FW_VERSION_LOAD_ADDRESS: usize = 64;
const OFF_FW_VERSION_SIZE: usize = 72;
const OFF_BOOT_PARAMS_LOAD_ADDRESS: usize = 76;
const OFF_API_VERSION: usize = 84;
const OFF_RUNTIME_SIZE: usize = 148;
const OFF_SHAVE_NN_FW_SIZE: usize = 152;

/// An interface version of the firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Api {
    /// Major.
    pub major: u16,
    /// Minor.
    pub minor: u16,
}

/// What the header of an NPU firmware file says (all addresses in the NPU's address space).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Where the image is loaded.
    pub load_address: u64,
    /// Size of the image in the file (from [`IMAGE_OFFSET`]).
    pub image_size: u64,
    /// Where the NPU's processor starts.
    pub entry_point: u64,
    /// Run-time memory at `load_address` (image, heap, stacks), without the two blocks below.
    pub runtime_size: u64,
    /// Where the driver puts the boot parameters (4 KiB).
    pub boot_params_address: u64,
    /// Where the driver puts the version block (4 KiB).
    pub version_address: u64,
    /// Size of the image of the SHAVE inference processors (0: none).
    pub shave_nn_size: u32,
    /// Boot interface version.
    pub boot_api: Api,
    /// Job interface version.
    pub jsm_api: Api,
    /// The firmware's version text.
    pub version: String,
}

fn u32_at(file: &[u8], off: usize) -> u32 {
    match file.get(off..off + 4) {
        Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        None => 0,
    }
}

fn u64_at(file: &[u8], off: usize) -> u64 {
    u32_at(file, off) as u64 | (u32_at(file, off + 4) as u64) << 32
}

fn api(file: &[u8], index: usize) -> Api {
    let v = u32_at(file, OFF_API_VERSION + index * 4);
    Api {
        major: (v >> 16) as u16,
        minor: v as u16,
    }
}

/// Where the image and its run-time memory lie: `(load address, image size, run-time size)`.
fn placement(file: &[u8], runtime: &Range) -> Result<(u64, u64, u64)> {
    let load_address = u64_at(file, OFF_IMAGE_LOAD_ADDRESS);
    let image_size = u32_at(file, OFF_IMAGE_SIZE) as u64;
    // the header's run-time size includes the boot parameter and version pages
    let runtime_size = (u32_at(file, OFF_RUNTIME_SIZE) as u64)
        .checked_sub(2 * 4096)
        .ok_or(Error::Firmware("run-time size too small"))?;
    if !runtime.holds(load_address, runtime_size) {
        return Err(Error::Firmware(
            "run-time memory outside the run-time range",
        ));
    }
    if (IMAGE_OFFSET as u64) + image_size > file.len() as u64 {
        return Err(Error::Firmware("image longer than the file"));
    }
    if !load_address.is_multiple_of(4096) || !runtime_size.is_multiple_of(4096) {
        return Err(Error::Firmware("run-time memory not page aligned"));
    }
    if runtime_size < image_size {
        return Err(Error::Firmware("run-time memory smaller than the image"));
    }
    Ok((load_address, image_size, runtime_size))
}

/// Where the driver puts the boot parameters and the version block (one page each).
fn side_pages(file: &[u8], runtime: &Range) -> Result<(u64, u64)> {
    let boot_params_address = u64_at(file, OFF_BOOT_PARAMS_LOAD_ADDRESS);
    if !runtime.holds(boot_params_address, 4096) {
        return Err(Error::Firmware(
            "boot parameters outside the run-time range",
        ));
    }
    let version_size = u32_at(file, OFF_FW_VERSION_SIZE);
    if version_size == 0 || version_size > 4096 {
        return Err(Error::Firmware("version block is not one page"));
    }
    let version_address = u64_at(file, OFF_FW_VERSION_LOAD_ADDRESS);
    if !runtime.holds(version_address, 4096) {
        return Err(Error::Firmware("version block outside the run-time range"));
    }
    Ok((boot_params_address, version_address))
}

/// The version text of the block after the header (at most 255 bytes).
fn version_text(file: &[u8]) -> String {
    let text = file.get(HEADER_SIZE..IMAGE_OFFSET).unwrap_or(&[]);
    let end = text
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(text.len())
        .min(255);
    String::from_utf8_lossy(&text[..end]).into_owned()
}

/// Parses and checks the header of an NPU firmware file for `model`.
pub fn parse(file: &[u8], model: &Model) -> Result<Image> {
    if file.len() <= IMAGE_OFFSET {
        return Err(Error::Firmware("file too small"));
    }
    if u32_at(file, OFF_HEADER_VERSION) != HEADER_VERSION {
        return Err(Error::Firmware("unknown header version"));
    }
    let (boot_params_address, version_address) = side_pages(file, &model.runtime)?;
    let (load_address, image_size, runtime_size) = placement(file, &model.runtime)?;
    let image = Range {
        start: load_address,
        end: load_address + image_size,
    };
    let entry_point = u64_at(file, OFF_ENTRY_POINT);
    if !image.holds(entry_point, 4096) {
        return Err(Error::Firmware("entry point outside the image"));
    }
    let shave_nn_size = u32_at(file, OFF_SHAVE_NN_FW_SIZE);
    if shave_nn_size > SHAVE_NN_MAX_SIZE {
        return Err(Error::Firmware("SHAVE image too big"));
    }
    let (boot_api, jsm_api) = (api(file, API_BOOT), api(file, API_JSM));
    if boot_api.major < MIN_API_MAJOR || jsm_api.major < MIN_API_MAJOR {
        return Err(Error::Firmware("boot or job interface older than 3.0"));
    }
    Ok(Image {
        load_address,
        image_size,
        entry_point,
        runtime_size,
        boot_params_address,
        version_address,
        shave_nn_size,
        boot_api,
        jsm_api,
        version: version_text(file),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ident::lookup;
    use alloc::vec::Vec;

    /// A well-formed file: 64 KiB image at the start of the run-time range, 16 MiB run time.
    fn file() -> Vec<u8> {
        let mut f = alloc::vec![0u8; IMAGE_OFFSET + 0x1_0000];
        let mut put = |off: usize, v: u32| f[off..off + 4].copy_from_slice(&v.to_le_bytes());
        put(OFF_HEADER_VERSION, 1);
        put(OFF_IMAGE_LOAD_ADDRESS, 0x8480_0000);
        put(OFF_IMAGE_SIZE, 0x1_0000);
        put(OFF_ENTRY_POINT, 0x8480_0400);
        put(OFF_FW_VERSION_LOAD_ADDRESS, 0x8580_1000);
        put(OFF_FW_VERSION_SIZE, 0x1000);
        put(OFF_BOOT_PARAMS_LOAD_ADDRESS, 0x8580_0000);
        put(OFF_API_VERSION, 3 << 16 | 28);
        put(OFF_API_VERSION + 16, 3 << 16 | 32);
        put(OFF_RUNTIME_SIZE, (16 << 20) + 0x2000);
        put(OFF_SHAVE_NN_FW_SIZE, 0x8_0000);
        f[HEADER_SIZE..HEADER_SIZE + 15].copy_from_slice(b"20250611-MTL_CL");
        f
    }

    fn with(off: usize, v: u32) -> Result<Image> {
        let mut f = file();
        f[off..off + 4].copy_from_slice(&v.to_le_bytes());
        parse(&f, &lookup(0x7d1d).unwrap())
    }

    #[test]
    fn parses_a_well_formed_file() {
        let img = parse(&file(), &lookup(0x7d1d).unwrap()).unwrap();
        assert_eq!(img.load_address, 0x8480_0000);
        assert_eq!(img.image_size, 0x1_0000);
        assert_eq!(img.entry_point, 0x8480_0400);
        assert_eq!(img.runtime_size, 16 << 20);
        assert_eq!(img.boot_params_address, 0x8580_0000);
        assert_eq!(img.version_address, 0x8580_1000);
        assert_eq!(
            img.boot_api,
            Api {
                major: 3,
                minor: 28
            }
        );
        assert_eq!(
            img.jsm_api,
            Api {
                major: 3,
                minor: 32
            }
        );
        assert_eq!(img.version, "20250611-MTL_CL");
        assert_eq!(img.shave_nn_size, 0x8_0000);
    }

    #[test]
    fn refuses_damaged_headers() {
        let m = lookup(0x7d1d).unwrap();
        assert_eq!(
            parse(&file()[..IMAGE_OFFSET], &m),
            Err(Error::Firmware("file too small"))
        );
        assert_eq!(
            with(OFF_HEADER_VERSION, 2),
            Err(Error::Firmware("unknown header version"))
        );
        assert_eq!(
            with(OFF_BOOT_PARAMS_LOAD_ADDRESS, 0x1000),
            Err(Error::Firmware(
                "boot parameters outside the run-time range"
            ))
        );
        assert_eq!(
            with(OFF_FW_VERSION_SIZE, 0x1001),
            Err(Error::Firmware("version block is not one page"))
        );
        assert_eq!(
            with(OFF_FW_VERSION_SIZE, 0),
            Err(Error::Firmware("version block is not one page"))
        );
        assert_eq!(
            with(OFF_FW_VERSION_LOAD_ADDRESS, 0x8880_0000),
            Err(Error::Firmware("version block outside the run-time range"))
        );
    }

    #[test]
    fn refuses_impossible_placements() {
        assert_eq!(
            with(OFF_RUNTIME_SIZE, 0x1000),
            Err(Error::Firmware("run-time size too small"))
        );
        assert_eq!(
            with(OFF_RUNTIME_SIZE, (64 << 20) + 0x3000),
            Err(Error::Firmware(
                "run-time memory outside the run-time range"
            ))
        );
        assert_eq!(
            with(OFF_IMAGE_SIZE, 0x1_0001),
            Err(Error::Firmware("image longer than the file"))
        );
        assert_eq!(
            with(OFF_IMAGE_LOAD_ADDRESS, 0x8480_0010),
            Err(Error::Firmware("run-time memory not page aligned"))
        );
        assert_eq!(
            with(OFF_RUNTIME_SIZE, 0x2000 + 0x1000),
            Err(Error::Firmware("run-time memory smaller than the image"))
        );
        assert_eq!(
            with(OFF_ENTRY_POINT, 0x8481_0000),
            Err(Error::Firmware("entry point outside the image"))
        );
        assert_eq!(
            with(OFF_SHAVE_NN_FW_SIZE, (2 << 20) + 1),
            Err(Error::Firmware("SHAVE image too big"))
        );
        assert_eq!(
            with(OFF_API_VERSION, 2 << 16 | 99),
            Err(Error::Firmware("boot or job interface older than 3.0"))
        );
    }

    #[test]
    fn version_text_without_terminator_is_bounded() {
        let mut f = file();
        for b in &mut f[HEADER_SIZE..IMAGE_OFFSET] {
            *b = b'x';
        }
        let img = parse(&f, &lookup(0x7d1d).unwrap()).unwrap();
        assert_eq!(img.version.len(), 255);
    }
}
