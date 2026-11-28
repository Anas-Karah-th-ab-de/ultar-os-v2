//! The binary LUKS2 header (4096 bytes, big endian) and its JSON area; both copies.

use crate::{Error, Reader, MAGIC};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Magic of the secondary header.
pub const MAGIC2: [u8; 6] = *b"SKUL\xba\xbe";
const BIN: usize = 4096;
const CSUM: usize = 448;
/// Allowed header sizes (binary header plus JSON area).
const SIZES: [u64; 9] = [0x4000, 0x8000, 0x1_0000, 0x2_0000, 0x4_0000, 0x8_0000, 0x10_0000, 0x20_0000, 0x40_0000];

pub struct Fields<'a> {
    pub magic: [u8; 6],
    pub seqid: u64,
    pub offset: u64,
    pub uuid: &'a str,
    pub salt: &'a [u8; 64],
}

/// Fills `h` (one whole header copy) with the binary header, `json` and the checksum.
pub fn write(h: &mut [u8], f: &Fields<'_>, json: &str) -> Result<(), Error> {
    let size = h.len();
    if json.len() >= size - BIN {
        return Err(Error::Header("JSON too large"));
    }
    h.fill(0);
    h[0..6].copy_from_slice(&f.magic);
    h[6..8].copy_from_slice(&2u16.to_be_bytes());
    h[8..16].copy_from_slice(&(size as u64).to_be_bytes());
    h[16..24].copy_from_slice(&f.seqid.to_be_bytes());
    h[72..78].copy_from_slice(b"sha256");
    h[104..168].copy_from_slice(f.salt);
    h[168..168 + f.uuid.len()].copy_from_slice(f.uuid.as_bytes());
    h[256..264].copy_from_slice(&f.offset.to_be_bytes());
    h[BIN..BIN + json.len()].copy_from_slice(json.as_bytes());
    let sum = Sha256::digest(&h[..]);
    h[CSUM..CSUM + 32].copy_from_slice(&sum);
    Ok(())
}

fn be64(b: &[u8], o: usize) -> u64 {
    u64::from_be_bytes(b[o..o + 8].try_into().unwrap_or([0; 8]))
}

/// Checks one header copy read at `at`: (seqid, metadata).
fn parse(h: &mut [u8], at: u64, magic: [u8; 6]) -> Result<(u64, Value), Error> {
    if h[0..6] != magic || h[6..8] != [0, 2] {
        return Err(Error::Header("magic"));
    }
    if be64(h, 8) != h.len() as u64 || be64(h, 256) != at {
        return Err(Error::Header("size or offset"));
    }
    if &h[72..79] != b"sha256\0" {
        return Err(Error::Unsupported("header checksum (only sha256)"));
    }
    let mut stored = [0u8; 32];
    stored.copy_from_slice(&h[CSUM..CSUM + 32]);
    h[CSUM..CSUM + 64].fill(0);
    if Sha256::digest(&h[..])[..] != stored {
        return Err(Error::Header("checksum"));
    }
    let area = &h[BIN..];
    let end = area.iter().position(|&b| b == 0).unwrap_or(area.len());
    let meta: Value = serde_json::from_slice(&area[..end]).map_err(|_| Error::Header("JSON"))?;
    Ok((be64(h, 16), meta))
}

fn copy_at(read: &mut Reader<'_>, at: u64, magic: [u8; 6]) -> Result<(u64, Value), Error> {
    let mut bin = vec![0u8; BIN];
    read(at, &mut bin)?;
    let size = be64(&bin, 8);
    if bin[0..6] != magic || !SIZES.contains(&size) {
        return Err(Error::Header("magic"));
    }
    let mut h = vec![0u8; size as usize];
    read(at, &mut h)?;
    parse(&mut h, at, magic)
}

/// The metadata of the valid copy with the higher sequence number.
pub fn read_best(read: &mut Reader<'_>) -> Result<Value, Error> {
    let mut head = [0u8; 8];
    read(0, &mut head)?;
    let primary = if head[..6] == MAGIC { Some(copy_at(read, 0, MAGIC)) } else { None };
    let secondary = SIZES.iter().find_map(|&at| copy_at(read, at, MAGIC2).ok());
    match (primary, secondary) {
        (None, None) => Err(Error::NotLuks),
        (Some(Ok((a, ma))), Some((b, mb))) => Ok(if b > a { mb } else { ma }),
        (Some(Ok((_, m))), None) | (_, Some((_, m))) => Ok(m),
        (Some(Err(e)), None) => Err(e),
    }
}
