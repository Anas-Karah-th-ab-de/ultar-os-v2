//! Length of a PE/COFF image (UEFI applications) from its headers.
//!
//! A kernel partition is larger than the image in it; the loader must hand the firmware
//! exactly the image. The length is the end of the furthest of: the headers, every section's
//! raw data, and the certificate table (signed images keep it at the end of the file).

/// Bytes of the image start the loader reads to find the length.
pub const HEADER_READ: usize = 64 * 1024;

/// Machine type the loader accepts: the processor it runs on.
#[cfg(target_arch = "aarch64")]
pub const MACHINE: u16 = 0xAA64;
/// Machine type the loader accepts: the processor it runs on.
#[cfg(not(target_arch = "aarch64"))]
pub const MACHINE: u16 = 0x8664;

/// Why an image is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeError {
    /// No `MZ` / `PE\0\0` signature, or headers outside the bytes read.
    Format,
    /// Not a PE32+ image for this processor.
    Machine,
    /// A size or offset does not fit.
    Bounds,
}

fn u16_at(b: &[u8], o: usize) -> Result<u16, PeError> {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(PeError::Format)
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, PeError> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(PeError::Format)
}

/// The file length of the image whose first bytes are `head` (at least the headers and the
/// section table; [`HEADER_READ`] bytes are always enough for the kernel).
pub fn image_len(head: &[u8], machine: u16) -> Result<u64, PeError> {
    if head.get(..2) != Some(b"MZ") {
        return Err(PeError::Format);
    }
    let pe = u32_at(head, 0x3C)? as usize;
    if head.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err(PeError::Format);
    }
    let coff = pe + 4;
    let sections = u16_at(head, coff + 2)? as usize;
    let opt_size = u16_at(head, coff + 16)? as usize;
    let opt = coff + 20;
    if u16_at(head, coff)? != machine || u16_at(head, opt)? != 0x20B {
        return Err(PeError::Machine);
    }
    let mut end = u64::from(u32_at(head, opt + 60)?);
    let dirs = u32_at(head, opt + 108)?;
    if dirs > 4 && opt_size >= 112 + 5 * 8 {
        let (offset, size) = (u32_at(head, opt + 112 + 32)?, u32_at(head, opt + 112 + 36)?);
        if size > 0 {
            end = end.max(u64::from(offset) + u64::from(size));
        }
    }
    let table = opt + opt_size;
    for i in 0..sections {
        let s = table + i * 40;
        let (size, ptr) = (u32_at(head, s + 16)?, u32_at(head, s + 20)?);
        if size > 0 {
            end = end.max(u64::from(ptr) + u64::from(size));
        }
    }
    if end == 0 {
        return Err(PeError::Bounds);
    }
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE32+ header with the given sections (raw size, raw pointer).
    pub fn image(machine: u16, sections: &[(u32, u32)], cert: Option<(u32, u32)>) -> Vec<u8> {
        let mut b = vec![0u8; 4096];
        b[..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84;
        b[coff..coff + 2].copy_from_slice(&machine.to_le_bytes());
        b[coff + 2..coff + 4].copy_from_slice(&(sections.len() as u16).to_le_bytes());
        b[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
        let opt = coff + 20;
        b[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        b[opt + 60..opt + 64].copy_from_slice(&0x400u32.to_le_bytes());
        b[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        if let Some((o, s)) = cert {
            b[opt + 144..opt + 148].copy_from_slice(&o.to_le_bytes());
            b[opt + 148..opt + 152].copy_from_slice(&s.to_le_bytes());
        }
        for (i, (size, ptr)) in sections.iter().enumerate() {
            let s = opt + 240 + i * 40;
            b[s + 16..s + 20].copy_from_slice(&size.to_le_bytes());
            b[s + 20..s + 24].copy_from_slice(&ptr.to_le_bytes());
        }
        b
    }

    #[test]
    fn length_is_the_furthest_end() {
        let img = image(
            MACHINE,
            &[(0x1000, 0x400), (0x200, 0x9000), (0, 0x20000)],
            None,
        );
        assert_eq!(image_len(&img, MACHINE), Ok(0x9200));
        let img = image(MACHINE, &[(0x1000, 0x400)], Some((0x5000, 0x800)));
        assert_eq!(image_len(&img, MACHINE), Ok(0x5800));
        let img = image(MACHINE, &[], None);
        assert_eq!(image_len(&img, MACHINE), Ok(0x400));
    }

    #[test]
    fn foreign_or_broken_images_are_refused() {
        let other = if MACHINE == 0x8664 { 0xAA64 } else { 0x8664 };
        assert_eq!(
            image_len(&image(other, &[(1, 1)], None), MACHINE),
            Err(PeError::Machine)
        );
        let mut img = image(MACHINE, &[(1, 1)], None);
        img[0x80] = b'X';
        assert_eq!(image_len(&img, MACHINE), Err(PeError::Format));
        assert_eq!(image_len(b"MZ", MACHINE), Err(PeError::Format));
        assert_eq!(image_len(&[0u8; 4096], MACHINE), Err(PeError::Format));
        let mut img = image(MACHINE, &[(1, 1)], None);
        img[0x3C..0x40].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        assert_eq!(image_len(&img, MACHINE), Err(PeError::Format));
        let img = image(MACHINE, &[(1, 1); 90], None);
        assert_eq!(image_len(&img[..1024], MACHINE), Err(PeError::Format));
        let mut img = image(MACHINE, &[], None);
        img[0x84 + 20 + 60..0x84 + 20 + 64].fill(0);
        assert_eq!(image_len(&img, MACHINE), Err(PeError::Bounds));
    }
}
