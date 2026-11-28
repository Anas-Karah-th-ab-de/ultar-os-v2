//! What a Windows installer database (`.msi`) says about itself: `ProductName`,
//! `ProductVersion` and `Manufacturer` from its `Property` table – read, never run.
//!
//! An `.msi` is a compound file (sectors, a FAT, a directory, a mini stream) holding
//! the database's tables as streams with encoded names; its texts live in a string
//! pool. Every chain is followed at most as many steps as the file has sectors, every
//! stream is capped ([`MAX_STREAM`]), and a damaged file is "not an installer", never a
//! panic (`tests::fuzz`).

use crate::pe::clean;
use crate::winread::{utf16_z, ReadAt};

/// Largest stream read (string data of large databases included).
const MAX_STREAM: usize = 32 << 20;
/// Most directory entries.
const MAX_ENTRIES: usize = 65536;
/// Most strings of the pool.
const MAX_STRINGS: usize = 1 << 20;
const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const SIGNATURE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// What the database says (unverified).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MsiInfo {
    /// `ProductName`.
    pub product: String,
    /// `ProductVersion`.
    pub version: String,
    /// `Manufacturer`.
    pub manufacturer: String,
}

struct Cfb<'a> {
    file: &'a dyn ReadAt,
    sector: u64,
    sectors: u64,
    fat: Vec<u32>,
    cutoff: u64,
    minifat: Vec<u32>,
    mini_sectors: Vec<u32>,
}

struct Entry {
    name: Vec<u16>,
    kind: u8,
    start: u32,
    size: u64,
}

/// Reads `file` as an installer database; `None`: not one (or too damaged).
pub fn read(file: &dyn ReadAt) -> Option<MsiInfo> {
    let (cfb, entries) = Cfb::open(file)?;
    let stream = |name: &str| {
        let wanted = encode(name);
        let e = entries.iter().find(|e| e.kind == 2 && e.name == wanted)?;
        cfb.stream(e)
    };
    let pool = stream("_StringPool")?;
    let data = stream("_StringData")?;
    let property = stream("Property")?;
    let strings = Strings::load(&pool, &data)?;
    let mut info = MsiInfo::default();
    let refs = strings.ref_bytes;
    let rows = property.len() / (2 * refs);
    for row in 0..rows {
        let key = strings.get(read_ref(&property, row * refs, refs)?);
        let value = strings.get(read_ref(&property, (rows + row) * refs, refs)?);
        match key.as_str() {
            "ProductName" => info.product = clean(&value),
            "ProductVersion" => info.version = clean(&value),
            "Manufacturer" => info.manufacturer = clean(&value),
            _ => {}
        }
    }
    Some(info)
}

fn read_ref(b: &[u8], at: usize, n: usize) -> Option<usize> {
    let s = b.get(at..at + n)?;
    let low = usize::from(u16::from_le_bytes([s[0], s[1]]));
    Some(if n == 3 {
        low | usize::from(s[2]) << 16
    } else {
        low
    })
}

impl<'a> Cfb<'a> {
    fn open(file: &'a dyn ReadAt) -> Option<(Self, Vec<Entry>)> {
        let h = file.bytes(0, 512)?;
        let u16_at = |at: usize| u16::from_le_bytes([h[at], h[at + 1]]);
        let u32_at = |at: usize| u32::from_le_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]]);
        if &h[..8] != SIGNATURE || u16_at(0x1C) != 0xFFFE || u16_at(0x20) != 6 {
            return None;
        }
        let shift = u16_at(0x1E);
        if shift != 9 && shift != 12 {
            return None;
        }
        let sector = 1u64 << shift;
        let sectors = file.len().saturating_sub(sector) / sector;
        let mut cfb = Self {
            file,
            sector,
            sectors,
            fat: Vec::new(),
            cutoff: u64::from(u32_at(0x38)).min(4096),
            minifat: Vec::new(),
            mini_sectors: Vec::new(),
        };
        cfb.fat = cfb.fat_sectors(&h, u32_at(0x2C), u32_at(0x44))?;
        let dir = cfb.chain(u32_at(0x30), MAX_ENTRIES as u64 * 128)?;
        let entries: Vec<Entry> = dir.chunks_exact(128).filter_map(entry).collect();
        let root = entries.first().filter(|e| e.kind == 5)?;
        let mini_len = root.size.min(MAX_STREAM as u64);
        cfb.mini_sectors = cfb.chain_sectors(root.start, mini_len)?;
        let minifat_len = u64::from(u32_at(0x40)).min(sectors) * sector;
        cfb.minifat = cfb.chain_sectors(u32_at(0x3C), minifat_len)?;
        Some((cfb, entries))
    }

    /// The FAT's own sectors: 109 in the header, the rest in the DIFAT chain.
    fn fat_sectors(&self, h: &[u8], count: u32, mut difat: u32) -> Option<Vec<u32>> {
        let count = usize::try_from(count).ok()?;
        let per = (self.sector / 4) as usize;
        if count as u64 > self.sectors / (per as u64) + 2 {
            return None;
        }
        let mut list: Vec<u32> = h[0x4C..0x200]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .take(count)
            .collect();
        let mut steps = 0u64;
        while list.len() < count {
            steps += 1;
            if steps > self.sectors {
                return None;
            }
            let at = self.offset(difat)?;
            for i in 0..per - 1 {
                if list.len() == count {
                    break;
                }
                list.push(self.file.u32_at(at + 4 * i as u64)?);
            }
            difat = self.file.u32_at(at + 4 * (per as u64 - 1))?;
        }
        Some(list)
    }

    fn offset(&self, n: u32) -> Option<u64> {
        (u64::from(n) < self.sectors).then(|| (u64::from(n) + 1) * self.sector)
    }

    /// The FAT entry of sector `n`.
    fn next(&self, n: u32) -> Option<u32> {
        let per = self.sector / 4;
        let fat = *self.fat.get(usize::try_from(u64::from(n) / per).ok()?)?;
        self.file
            .u32_at(self.offset(fat)? + 4 * (u64::from(n) % per))
    }

    /// The sectors of the chain at `start` for `len` bytes (no more steps than sectors).
    fn chain_sectors(&self, mut n: u32, len: u64) -> Option<Vec<u32>> {
        let want = len.div_ceil(self.sector);
        let mut out = Vec::new();
        while (out.len() as u64) < want {
            if n == END_OF_CHAIN || out.len() as u64 >= self.sectors {
                break;
            }
            self.offset(n)?;
            out.push(n);
            n = self.next(n)?;
        }
        Some(out)
    }

    /// The bytes of the chain at `start`, at most `len`.
    fn chain(&self, start: u32, len: u64) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for n in self.chain_sectors(start, len)? {
            out.extend(self.file.bytes(self.offset(n)?, self.sector as usize)?);
        }
        out.truncate(usize::try_from(len).ok()?);
        Some(out)
    }

    fn stream(&self, e: &Entry) -> Option<Vec<u8>> {
        let size = usize::try_from(e.size).ok().filter(|s| *s <= MAX_STREAM)?;
        let data = if e.size < self.cutoff {
            self.mini(e.start, size)?
        } else {
            self.chain(e.start, e.size)?
        };
        (data.len() == size).then_some(data)
    }

    /// A stream of the mini stream (64-byte sectors).
    fn mini(&self, mut n: u32, size: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let limit = self.mini_sectors.len() as u64 * self.sector / 64;
        let mut steps = 0u64;
        while out.len() < size {
            steps += 1;
            if n == END_OF_CHAIN || steps > limit {
                return None;
            }
            let at = u64::from(n) * 64;
            let host = *self
                .mini_sectors
                .get(usize::try_from(at / self.sector).ok()?)?;
            out.extend(self.file.bytes(self.offset(host)? + at % self.sector, 64)?);
            n = self.mini_next(n)?;
        }
        out.truncate(size);
        Some(out)
    }

    fn mini_next(&self, n: u32) -> Option<u32> {
        let per = self.sector / 4;
        let s = *self
            .minifat
            .get(usize::try_from(u64::from(n) / per).ok()?)?;
        self.file.u32_at(self.offset(s)? + 4 * (u64::from(n) % per))
    }
}

fn entry(b: &[u8]) -> Option<Entry> {
    let name_len = usize::from(u16::from_le_bytes([b[64], b[65]]));
    if name_len < 2 || name_len > 64 {
        return None;
    }
    let (text, _) = utf16_z(&b[..name_len], 0, 31)?;
    let low = u64::from(u32::from_le_bytes([b[120], b[121], b[122], b[123]]));
    Some(Entry {
        name: text.encode_utf16().collect(),
        kind: b[66],
        start: u32::from_le_bytes([b[116], b[117], b[118], b[119]]),
        // version 3 files leave the high half undefined: only the low half counts
        size: low,
    })
}

/// The stream name of table `name` (`!`-prefixed, two characters per unit).
fn encode(name: &str) -> Vec<u16> {
    fn mime(c: u8) -> Option<u16> {
        Some(u16::from(match c {
            b'0'..=b'9' => c - b'0',
            b'A'..=b'Z' => c - b'A' + 10,
            b'a'..=b'z' => c - b'a' + 36,
            b'.' => 62,
            b'_' => 63,
            _ => return None,
        }))
    }
    let b = name.as_bytes();
    let mut out = vec![0x4840];
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        i += 1;
        match mime(c) {
            Some(m1) => match b.get(i).and_then(|&n| mime(n)) {
                Some(m2) => {
                    out.push(0x3800 + m1 + (m2 << 6));
                    i += 1;
                }
                None => out.push(0x4800 + m1),
            },
            None => out.push(u16::from(c)),
        }
    }
    out
}

struct Strings {
    data: Vec<u8>,
    spans: Vec<(usize, usize)>,
    utf8: bool,
    ref_bytes: usize,
}

impl Strings {
    /// The pool as the database engine reads it (ids from 1; a zero entry is an empty id;
    /// a long string keeps the high half of its length in the next entry).
    fn load(pool: &[u8], data: &[u8]) -> Option<Self> {
        let words: Vec<u16> = pool
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let head = u32::from(*words.first()?) | u32::from(*words.get(1)?) << 16;
        let count = words.len() / 2;
        let mut spans = vec![(0, 0)];
        let (mut i, mut offset) = (1usize, 0usize);
        while i < count && spans.len() < MAX_STRINGS {
            let (len, refs) = (usize::from(words[i * 2]), words[i * 2 + 1]);
            let len = if len == 0 && refs == 0 {
                i += 1;
                spans.push((0, 0));
                continue;
            } else if len == 0 {
                let hi = usize::from(*words.get(i * 2 + 3)?);
                let lo = usize::from(*words.get(i * 2 + 2)?);
                i += 2;
                (hi << 16) + lo
            } else {
                i += 1;
                len
            };
            if offset + len > data.len() {
                break;
            }
            spans.push((offset, len));
            offset += len;
        }
        Some(Self {
            data: data.to_vec(),
            spans,
            utf8: head & 0x7fff_ffff == 65001,
            ref_bytes: if head & 0x8000_0000 != 0 { 3 } else { 2 },
        })
    }

    fn get(&self, id: usize) -> String {
        let Some(&(at, len)) = self.spans.get(id) else {
            return String::new();
        };
        let bytes = &self.data[at..at + len];
        match std::str::from_utf8(bytes) {
            Ok(s) if self.utf8 || s.is_ascii() => s.to_owned(),
            Ok(s) => s.to_owned(),
            // a single-byte code page: Latin-1 is close enough for a name shown as unverified
            Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compound file (version 3) whose small streams all live in the mini stream.
    pub(crate) fn compound(streams: &[(Vec<u16>, Vec<u8>)]) -> Vec<u8> {
        const FREE: u32 = 0xFFFF_FFFF;
        const FAT_SECTOR: u32 = 0xFFFF_FFFD;
        let mut mini = Vec::new();
        let mut minifat: Vec<u32> = Vec::new();
        let mut starts = Vec::new();
        for (_, data) in streams {
            let first = (mini.len() / 64) as u32;
            starts.push(first);
            let n = data.len().div_ceil(64).max(1);
            for k in 0..n {
                let last = k + 1 == n;
                minifat.push(if last {
                    END_OF_CHAIN
                } else {
                    first + k as u32 + 1
                });
            }
            mini.extend_from_slice(data);
            mini.resize(mini.len().div_ceil(64).max(1) * 64, 0);
        }
        let mini_sectors = mini.len().div_ceil(512);
        mini.resize(mini_sectors * 512, 0);
        // sector 0 FAT, 1 directory, 2 mini FAT, 3.. mini stream
        let mut fat = vec![FAT_SECTOR, END_OF_CHAIN, END_OF_CHAIN];
        for k in 0..mini_sectors {
            fat.push(if k + 1 == mini_sectors {
                END_OF_CHAIN
            } else {
                4 + k as u32
            });
        }
        fat.resize(128, FREE);
        let mut h = vec![0u8; 512];
        h[..8].copy_from_slice(SIGNATURE);
        h[0x18..0x1A].copy_from_slice(&0x3Eu16.to_le_bytes());
        h[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
        h[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
        h[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
        h[0x20..0x22].copy_from_slice(&6u16.to_le_bytes());
        h[0x2C..0x30].copy_from_slice(&1u32.to_le_bytes());
        h[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
        h[0x38..0x3C].copy_from_slice(&4096u32.to_le_bytes());
        h[0x3C..0x40].copy_from_slice(&2u32.to_le_bytes());
        h[0x40..0x44].copy_from_slice(&1u32.to_le_bytes());
        h[0x44..0x48].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
        for i in 0..109 {
            let v = if i == 0 { 0 } else { FREE };
            h[0x4C + 4 * i..0x50 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        let mut dir = Vec::new();
        let mut put = |name: &[u16], kind: u8, start: u32, size: u32, child: u32, right: u32| {
            let mut e = vec![0u8; 128];
            for (i, u) in name.iter().enumerate().take(31) {
                e[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
            }
            let len = ((name.len().min(31) + 1) * 2) as u16;
            e[64..66].copy_from_slice(&len.to_le_bytes());
            e[66] = kind;
            e[68..72].copy_from_slice(&FREE.to_le_bytes());
            e[72..76].copy_from_slice(&right.to_le_bytes());
            e[76..80].copy_from_slice(&child.to_le_bytes());
            e[116..120].copy_from_slice(&start.to_le_bytes());
            e[120..124].copy_from_slice(&size.to_le_bytes());
            dir.extend(e);
        };
        let root: Vec<u16> = "Root Entry".encode_utf16().collect();
        put(&root, 5, 3, (mini_sectors * 512) as u32, 1, FREE);
        for (i, (name, data)) in streams.iter().enumerate() {
            let right = if i + 1 < streams.len() {
                i as u32 + 2
            } else {
                FREE
            };
            put(name, 2, starts[i], data.len() as u32, FREE, right);
        }
        dir.resize(512, 0);
        let mut out = h;
        out.extend(fat.iter().flat_map(|v| v.to_le_bytes()));
        out.extend(dir);
        let mut mf: Vec<u8> = minifat.iter().flat_map(|v| v.to_le_bytes()).collect();
        mf.resize(512, 0xFF);
        out.extend(mf);
        out.extend(mini);
        out
    }

    pub(crate) fn sample(props: &[(&str, &str)]) -> Vec<u8> {
        let mut texts: Vec<&str> = Vec::new();
        for (k, v) in props {
            texts.push(k);
            texts.push(v);
        }
        let mut pool: Vec<u8> = Vec::new();
        pool.extend(65001u32.to_le_bytes());
        let mut data = Vec::new();
        for t in &texts {
            pool.extend((t.len() as u16).to_le_bytes());
            pool.extend(1u16.to_le_bytes());
            data.extend_from_slice(t.as_bytes());
        }
        let mut table = Vec::new();
        for i in 0..props.len() {
            table.extend(((2 * i + 1) as u16).to_le_bytes());
        }
        for i in 0..props.len() {
            table.extend(((2 * i + 2) as u16).to_le_bytes());
        }
        compound(&[
            (encode("_StringPool"), pool),
            (encode("_StringData"), data),
            (encode("Property"), table),
        ])
    }

    #[test]
    fn the_property_table_is_read() {
        let msi = sample(&[
            ("ALLUSERS", "1"),
            ("Manufacturer", "Igor Pavlov"),
            ("ProductName", "7-Zip 24.09 (x64 edition)"),
            ("ProductVersion", "24.09.00.0"),
        ]);
        let info = read(&msi.as_slice()).unwrap();
        assert_eq!(info.product, "7-Zip 24.09 (x64 edition)");
        assert_eq!(info.version, "24.09.00.0");
        assert_eq!(info.manufacturer, "Igor Pavlov");
    }

    #[test]
    fn names_are_encoded_like_the_database_engine() {
        assert_eq!(encode("Property")[0], 0x4840);
        // "Pr" → 0x3800 + m('P') + m('r') << 6
        assert_eq!(encode("Property")[1], 0x3800 + 25 + (53 << 6));
        assert_eq!(encode("_StringPool").len(), 7);
    }

    #[test]
    fn not_installers() {
        assert!(read(&&b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1"[..]).is_none());
        assert!(read(&&[0u8; 2048][..]).is_none());
        let only = compound(&[(encode("Property"), vec![1, 0, 2, 0])]);
        assert!(read(&only.as_slice()).is_none());
    }

    #[test]
    fn fuzz() {
        let base = sample(&[("ProductName", "P"), ("Manufacturer", "M")]);
        let mut rng = crate::winread::Rng(0xfee1_600d);
        for _ in 0..4000 {
            let mut f = base.clone();
            for _ in 0..1 + rng.below(12) {
                let at = rng.below(f.len());
                f[at] = rng.next() as u8;
            }
            if rng.below(8) == 0 {
                f.truncate(rng.below(f.len()));
            }
            if let Some(info) = read(&f.as_slice()) {
                assert!(info.product.chars().count() <= crate::pe::MAX_TEXT);
            }
        }
    }
}
