//! The container of the GuC firmware: a 128-byte CSS header, the code, then the RSA signature.
//! Layout and checks follow `abi/uc_fw_abi.h` and `xe_uc_fw.c` of the Linux `xe` driver (MIT).
//!
//! ```text
//! | CSS header (128 bytes) | uCode | RSA key | modulus | exponent |
//! ```
//! The hardware copies header + uCode in one DMA transfer; the RSA key is handed over separately.

use crate::{Error, Result};

/// Size of the CSS header.
pub const CSS_HEADER_SIZE: u32 = 128;
/// Oldest GuC release this driver's host interface works with (as Linux: 70.29.2).
pub const GUC_MIN: Version = Version {
    major: 70,
    minor: 29,
    patch: 2,
};
/// File name of the GuC firmware for Meteor Lake / Arrow Lake (linux-firmware, `i915/`).
pub const GUC_FILE_MTL: &str = "mtl_guc_70.bin";

/// A firmware version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Major: the host interface generation.
    pub major: u8,
    /// Minor.
    pub minor: u8,
    /// Patch.
    pub patch: u8,
}

impl Version {
    fn from_css(v: u32) -> Version {
        Version {
            major: (v >> 16) as u8,
            minor: (v >> 8) as u8,
            patch: v as u8,
        }
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// How the firmware was built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    /// Production build (the only kind production hardware accepts).
    Production,
    /// Pre-production build.
    PreProduction,
    /// Debug build.
    Debug,
    /// A value this driver does not know.
    Unknown(u32),
}

/// What the header of a GuC firmware file says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GucImage {
    /// Size of the code after the header (bytes).
    pub ucode_size: u32,
    /// Size of the RSA key (bytes); more than 256 means it is handed over through the GGTT.
    pub rsa_size: u32,
    /// Release version of the firmware.
    pub release: Version,
    /// Version of the submission interface.
    pub submission: Version,
    /// Build kind.
    pub build: Build,
    /// Memory the firmware wants for its private data (bytes).
    pub private_data_size: u32,
}

impl GucImage {
    /// Bytes the DMA transfer copies: header and code.
    pub fn upload_size(&self) -> u32 {
        CSS_HEADER_SIZE + self.ucode_size
    }

    /// Where the RSA key starts in the file.
    pub fn rsa_offset(&self) -> u32 {
        CSS_HEADER_SIZE + self.ucode_size
    }

    /// The first 256 bytes of the RSA key as 64 little-endian words (keys of at most 256 bytes
    /// are written into registers).
    pub fn rsa_words(&self, file: &[u8]) -> Result<[u32; 64]> {
        let start = self.rsa_offset() as usize;
        let key = file
            .get(start..start + 256)
            .ok_or(Error::Firmware("RSA key shorter than 256 bytes"))?;
        let mut out = [0u32; 64];
        for (w, c) in out.iter_mut().zip(key.chunks_exact(4)) {
            *w = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        Ok(out)
    }
}

fn word(file: &[u8], off: usize) -> u32 {
    match file.get(off..off + 4) {
        Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        None => 0,
    }
}

/// Parses and checks the header of a GuC firmware file.
pub fn parse_guc(file: &[u8]) -> Result<GucImage> {
    if file.len() < CSS_HEADER_SIZE as usize {
        return Err(Error::Firmware("file shorter than the CSS header"));
    }
    let header_size_dw = word(file, 4);
    let size_dw = word(file, 24);
    let (key_dw, modulus_dw, exponent_dw) = (word(file, 28), word(file, 32), word(file, 36));
    // the header size counts the CSS header and the three RSA parts
    let css_dw = header_size_dw
        .checked_sub(key_dw)
        .and_then(|v| v.checked_sub(modulus_dw))
        .and_then(|v| v.checked_sub(exponent_dw));
    if css_dw != Some(CSS_HEADER_SIZE / 4) {
        return Err(Error::Firmware("unexpected CSS header size"));
    }
    let ucode_size = size_dw
        .checked_sub(header_size_dw)
        .and_then(|dw| dw.checked_mul(4))
        .ok_or(Error::Firmware("code size out of range"))?;
    let rsa_size = key_dw
        .checked_mul(4)
        .ok_or(Error::Firmware("RSA key size out of range"))?;
    let need = (CSS_HEADER_SIZE as u64) + ucode_size as u64 + rsa_size as u64;
    if (file.len() as u64) < need {
        return Err(Error::Firmware(
            "file shorter than header, code and RSA key",
        ));
    }
    if ucode_size == 0 || rsa_size == 0 {
        return Err(Error::Firmware("no code or no RSA key"));
    }
    let release = Version::from_css(word(file, 64));
    if release < GUC_MIN {
        return Err(Error::Firmware("GuC older than 70.29.2"));
    }
    let build = match (word(file, 124) >> 2) & 0x3 {
        0 => Build::Production,
        1 => Build::PreProduction,
        2 => Build::Debug,
        v => Build::Unknown(v),
    };
    Ok(GucImage {
        ucode_size,
        rsa_size,
        release,
        submission: Version::from_css(word(file, 68)),
        build,
        private_data_size: word(file, 120),
    })
}

/// A firmware file for the tests: `ucode` bytes of code and an RSA key of `key_dw` words.
#[cfg(test)]
pub fn test_image(ucode: usize, key_dw: u32, release: (u8, u8, u8)) -> alloc::vec::Vec<u8> {
    let mut f = alloc::vec![0u8; 128 + ucode + key_dw as usize * 4];
    let mut put = |off: usize, v: u32| f[off..off + 4].copy_from_slice(&v.to_le_bytes());
    let (modulus_dw, exponent_dw) = (key_dw, 1);
    let header_dw = 32 + key_dw + modulus_dw + exponent_dw;
    put(0, 6); // module type
    put(4, header_dw);
    put(24, header_dw + ucode as u32 / 4);
    put(28, key_dw);
    put(32, modulus_dw);
    put(36, exponent_dw);
    put(
        64,
        (release.0 as u32) << 16 | (release.1 as u32) << 8 | release.2 as u32,
    );
    put(68, 1 << 16 | 19 << 8);
    put(120, 0x4_0000);
    for (i, b) in f[128 + ucode..].iter_mut().enumerate() {
        *b = i as u8;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_meteor_lake_style_image() {
        // Meteor Lake's GuC carries a 384-byte key (96 words)
        let f = test_image(0x5_0000, 96, (70, 53, 0));
        let img = parse_guc(&f).unwrap();
        assert_eq!(img.ucode_size, 0x5_0000);
        assert_eq!(img.rsa_size, 384);
        assert_eq!(img.upload_size(), 0x5_0080);
        assert_eq!(img.rsa_offset(), 0x5_0080);
        assert_eq!(
            img.release,
            Version {
                major: 70,
                minor: 53,
                patch: 0
            }
        );
        assert_eq!(
            img.submission,
            Version {
                major: 1,
                minor: 19,
                patch: 0
            }
        );
        assert_eq!(img.build, Build::Production);
        assert_eq!(img.private_data_size, 0x4_0000);
        assert_eq!(alloc::format!("{}", img.release), "70.53.0");
        let rsa = img.rsa_words(&f).unwrap();
        assert_eq!(rsa[0], u32::from_le_bytes([0, 1, 2, 3]));
        assert_eq!(rsa[63], u32::from_le_bytes([252, 253, 254, 255]));
    }

    #[test]
    fn refuses_damaged_files() {
        let good = test_image(0x1000, 64, (70, 53, 0));
        assert!(parse_guc(&good).is_ok());
        assert_eq!(
            parse_guc(&good[..100]),
            Err(Error::Firmware("file shorter than the CSS header"))
        );
        assert_eq!(
            parse_guc(&good[..good.len() - 1]),
            Err(Error::Firmware(
                "file shorter than header, code and RSA key"
            ))
        );
        let mut bad = good.clone();
        bad[4..8].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(
            parse_guc(&bad),
            Err(Error::Firmware("unexpected CSS header size"))
        );
        let mut bad = good.clone();
        bad[24..28].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            parse_guc(&bad),
            Err(Error::Firmware("code size out of range"))
        );
        let mut bad = good.clone();
        bad[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_guc(&bad).is_err());
        assert_eq!(
            parse_guc(&test_image(0x1000, 64, (70, 29, 1))),
            Err(Error::Firmware("GuC older than 70.29.2"))
        );
        assert_eq!(
            parse_guc(&test_image(0, 64, (70, 53, 0))),
            Err(Error::Firmware("no code or no RSA key"))
        );
    }

    #[test]
    fn rsa_words_need_256_bytes() {
        let f = test_image(0x1000, 32, (70, 53, 0));
        let img = parse_guc(&f).unwrap();
        assert_eq!(
            img.rsa_words(&f),
            Err(Error::Firmware("RSA key shorter than 256 bytes"))
        );
    }
}
