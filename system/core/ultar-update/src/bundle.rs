//! The bundle file `.uup`: optional key rotations, the signed manifest, then the payloads.
//!
//! A bundle carries **no file names and no paths**; payloads are identified by their position,
//! which the signed manifest defines. Every length is checked against a fixed limit before any
//! memory is allocated for it. Layout: `docs/SYSTEM-UPDATE.md` §2.4.

use crate::error::{Malformed, UpdateError};
use crate::keys::{MAX_CHAIN, MAX_ROTATION};
use crate::manifest::MAX_MANIFEST;
use crate::sign::{self, ENVELOPE_OVERHEAD, KEYS_MAGIC, MANIFEST_MAGIC};
use std::io::{self, Read, Write};

/// Magic bytes of a bundle.
pub const BUNDLE_MAGIC: [u8; 4] = *b"UUP1";
/// Largest rotation section in bytes.
pub const MAX_ROTATION_SECTION: usize = 64 * 1024;
/// Largest header (everything before the payloads) in bytes.
pub const MAX_HEADER: usize = 128 * 1024;
/// File-name ending of bundles.
pub const BUNDLE_SUFFIX: &str = ".uup";

/// The part of a bundle before the payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Signed key rotations, in the order they must be offered to the trust store.
    pub rotations: Vec<Vec<u8>>,
    /// The signed manifest envelope.
    pub signed_manifest: Vec<u8>,
    /// The exact header bytes as they stand in the bundle.
    pub raw: Vec<u8>,
}

fn read_exact(reader: &mut dyn Read, buf: &mut [u8]) -> Result<(), UpdateError> {
    reader.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => UpdateError::Malformed(Malformed::Length),
        _ => UpdateError::Io(e),
    })
}

fn split_rotations(mut section: &[u8]) -> Result<Vec<Vec<u8>>, Malformed> {
    let mut rotations = Vec::new();
    while !section.is_empty() {
        if rotations.len() == MAX_CHAIN {
            return Err(Malformed::Length);
        }
        let envelope = sign::open(&KEYS_MAGIC, section, MAX_ROTATION)?;
        rotations.push(section[..envelope.len].to_vec());
        section = &section[envelope.len..];
    }
    Ok(rotations)
}

impl Header {
    /// Builds a header from its parts (the signing tool's side).
    pub fn new(rotations: &[Vec<u8>], signed_manifest: &[u8]) -> Result<Self, Malformed> {
        let section: Vec<u8> = rotations.concat();
        if section.len() > MAX_ROTATION_SECTION {
            return Err(Malformed::Length);
        }
        // Both parts must read back the way a device reads them.
        if split_rotations(&section)? != rotations {
            return Err(Malformed::Field);
        }
        sign::open_exact(&MANIFEST_MAGIC, signed_manifest, MAX_MANIFEST)?;
        let mut raw = Vec::with_capacity(8 + section.len() + signed_manifest.len());
        raw.extend_from_slice(&BUNDLE_MAGIC);
        raw.extend_from_slice(&(section.len() as u32).to_le_bytes());
        raw.extend_from_slice(&section);
        raw.extend_from_slice(signed_manifest);
        Ok(Self {
            rotations: rotations.to_vec(),
            signed_manifest: signed_manifest.to_vec(),
            raw,
        })
    }

    /// Reads exactly the header from the start of a bundle stream and leaves the reader at the
    /// first payload byte. Nothing in it is verified yet.
    pub fn read(reader: &mut dyn Read) -> Result<Self, UpdateError> {
        let mut fixed = [0u8; 8];
        read_exact(reader, &mut fixed)?;
        if fixed[..4] != BUNDLE_MAGIC {
            return Err(Malformed::Magic.into());
        }
        let section_len = u32::from_le_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]) as usize;
        if section_len > MAX_ROTATION_SECTION {
            return Err(Malformed::Length.into());
        }
        let mut section = vec![0u8; section_len];
        read_exact(reader, &mut section)?;
        let rotations = split_rotations(&section)?;
        let mut start = [0u8; 6];
        read_exact(reader, &mut start)?;
        if start[..4] != MANIFEST_MAGIC {
            return Err(Malformed::Magic.into());
        }
        let body_len = usize::from(u16::from_le_bytes([start[4], start[5]]));
        if body_len == 0 || body_len > MAX_MANIFEST {
            return Err(Malformed::Length.into());
        }
        let mut signed_manifest = vec![0u8; body_len + ENVELOPE_OVERHEAD];
        signed_manifest[..6].copy_from_slice(&start);
        read_exact(reader, &mut signed_manifest[6..])?;
        let mut raw = fixed.to_vec();
        raw.extend_from_slice(&section);
        raw.extend_from_slice(&signed_manifest);
        Ok(Self {
            rotations,
            signed_manifest,
            raw,
        })
    }

    /// Length of the header in bytes (offset of the first payload).
    pub fn len(&self) -> u64 {
        self.raw.len() as u64
    }

    /// Always false: a header has at least its fixed part. Present for API completeness.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

/// Writes a complete bundle: header, then each payload stream in manifest order. The payload
/// sizes are **not** checked here; `uupdate-sign` verifies the result like a device would.
/// Returns the number of bytes written.
pub fn write_bundle(
    out: &mut dyn Write,
    header: &Header,
    payloads: &mut [&mut dyn Read],
) -> io::Result<u64> {
    out.write_all(&header.raw)?;
    let mut total = header.len();
    for payload in payloads.iter_mut() {
        total += io::copy(payload, out)?;
    }
    out.flush()?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{KeySet, Rotation};
    use crate::sign::{public_of, seal, MANIFEST_DOMAIN};

    const SECRET: [u8; 32] = [5; 32];

    fn signed_manifest(body: &[u8]) -> Vec<u8> {
        seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, body, &SECRET).unwrap()
    }

    fn rotation(serial: u64) -> Vec<u8> {
        Rotation {
            set: KeySet {
                serial,
                keys: vec![public_of(&SECRET)],
            },
            created: 1,
        }
        .sign(&SECRET)
        .unwrap()
    }

    #[test]
    fn header_round_trip_leaves_reader_at_payload() {
        let rotations = vec![rotation(1), rotation(2)];
        let header = Header::new(&rotations, &signed_manifest(b"manifest")).unwrap();
        let mut file = Vec::new();
        let mut p1: &[u8] = b"KERNEL";
        let mut p2: &[u8] = b"SYSTEM!";
        let total = write_bundle(&mut file, &header, &mut [&mut p1, &mut p2]).unwrap();
        assert_eq!(total, file.len() as u64);
        let mut reader: &[u8] = &file;
        let read = Header::read(&mut reader).unwrap();
        assert_eq!(read, header);
        assert_eq!(reader, b"KERNELSYSTEM!");
        assert_eq!(read.len(), (file.len() - 13) as u64);
        assert!(!read.is_empty());
    }

    #[test]
    fn header_without_rotations() {
        let header = Header::new(&[], &signed_manifest(b"m")).unwrap();
        assert_eq!(&header.raw[..8], b"UUP1\0\0\0\0");
        let mut reader: &[u8] = &header.raw;
        assert!(Header::read(&mut reader).unwrap().rotations.is_empty());
    }

    #[test]
    fn hostile_headers_are_refused_before_allocation() {
        let good = Header::new(&[rotation(1)], &signed_manifest(b"m"))
            .unwrap()
            .raw;
        let mut wrong_magic = good.clone();
        wrong_magic[0] = b'X';
        let mut huge_section = good.clone();
        huge_section[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut section_too_long = good.clone();
        section_too_long[4..8].copy_from_slice(&(MAX_ROTATION_SECTION as u32 + 1).to_le_bytes());
        let mut section_misaligned = good.clone();
        let len = u32::from_le_bytes([good[4], good[5], good[6], good[7]]);
        section_misaligned[4..8].copy_from_slice(&(len - 1).to_le_bytes());
        let mut bad_manifest_magic = good.clone();
        let at = 8 + len as usize;
        bad_manifest_magic[at] = b'X';
        let mut zero_manifest = good.clone();
        zero_manifest[at + 4] = 0;
        zero_manifest[at + 5] = 0;
        let mut big_manifest = good.clone();
        big_manifest[at + 4..at + 6].copy_from_slice(&u16::MAX.to_le_bytes());
        for (name, bytes) in [
            ("magic", wrong_magic),
            ("huge", huge_section),
            ("too long", section_too_long),
            ("misaligned", section_misaligned),
            ("manifest magic", bad_manifest_magic),
            ("zero manifest", zero_manifest),
            ("big manifest", big_manifest),
        ] {
            let mut reader: &[u8] = &bytes;
            assert!(Header::read(&mut reader).is_err(), "{name}");
        }
        for cut in 0..good.len() {
            let mut reader: &[u8] = &good[..cut];
            assert!(
                matches!(
                    Header::read(&mut reader),
                    Err(UpdateError::Malformed(Malformed::Length))
                ),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn builder_refuses_what_a_device_would_refuse() {
        let manifest = signed_manifest(b"m");
        assert!(Header::new(&[b"garbage".to_vec()], &manifest).is_err());
        assert!(Header::new(&[], b"not an envelope").is_err());
        let mut joined = rotation(1);
        joined.extend(rotation(2));
        assert_eq!(Header::new(&[joined], &manifest), Err(Malformed::Field));
        let many: Vec<Vec<u8>> = (0..=MAX_CHAIN as u64).map(rotation).collect();
        assert!(Header::new(&many, &manifest).is_err());
    }
}
