//! The resource table (`resources.arsc`), read only to resolve the references of the
//! manifest: the app's name (a string, by language) and its symbol (file paths of the
//! launcher icon by screen density).

use super::axml::Pool;

/// Largest table the reader takes.
pub const MAX_TABLE: usize = 64 << 20;
/// Most packages of one table.
const MAX_PACKAGES: usize = 16;
/// Most entries of one type chunk.
const MAX_ENTRIES: usize = 1 << 16;

const RES_STRING_POOL: u16 = 0x0001;
const RES_TABLE: u16 = 0x0002;
const RES_TABLE_PACKAGE: u16 = 0x0200;
const RES_TABLE_TYPE: u16 = 0x0201;

const FLAG_COMPLEX: u16 = 0x0001;
const FLAG_COMPACT: u16 = 0x0008;
const TYPE_SPARSE: u8 = 0x01;
const TYPE_OFFSET16: u8 = 0x02;
const NO_ENTRY: u32 = 0xffff_ffff;

/// One value of a resource in one configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Language of the configuration (`""` = default, `"ar"` …).
    pub language: String,
    /// Screen density (dpi; 0 = default, 0xfffe = any, 0xffff = none).
    pub density: u16,
    /// The value.
    pub value: Resolved,
}

/// A value as the table has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A string of the global pool (also file paths of drawables).
    Text(String),
    /// Another resource.
    Ref(u32),
    /// Anything else.
    Other,
}

/// Why the table cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArscError;

fn u16_at(b: &[u8], at: usize) -> Result<u16, ArscError> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(ArscError)
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, ArscError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(ArscError)
}

/// The table.
pub struct Table<'a> {
    bytes: &'a [u8],
    pool: Pool,
    /// (package id, offset, size) of each package chunk.
    packages: Vec<(u32, usize, usize)>,
}

impl<'a> Table<'a> {
    /// Reads the table's outline (the values are looked up on demand).
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ArscError> {
        if bytes.len() > MAX_TABLE || u16_at(bytes, 0)? != RES_TABLE {
            return Err(ArscError);
        }
        let mut at = u16_at(bytes, 2)? as usize;
        let mut pool = None;
        let mut packages = Vec::new();
        while at + 8 <= bytes.len() {
            let kind = u16_at(bytes, at)?;
            let size = u32_at(bytes, at + 4)? as usize;
            if size < 8 || at + size > bytes.len() {
                return Err(ArscError);
            }
            match kind {
                RES_STRING_POOL if pool.is_none() => {
                    pool = Some(Pool::parse(&bytes[at..at + size]).map_err(|_| ArscError)?);
                }
                RES_TABLE_PACKAGE => {
                    if packages.len() >= MAX_PACKAGES {
                        return Err(ArscError);
                    }
                    packages.push((u32_at(bytes, at + 8)?, at, size));
                }
                _ => {}
            }
            at += size;
        }
        Ok(Self {
            bytes,
            pool: pool.ok_or(ArscError)?,
            packages,
        })
    }

    /// Every value of resource `id`, in every configuration.
    pub fn lookup(&self, id: u32) -> Vec<Found> {
        let (pkg, kind, entry) = (id >> 24, ((id >> 16) & 0xff) as u8, id & 0xffff);
        let mut out = Vec::new();
        for &(pid, at, size) in &self.packages {
            if pid != pkg {
                continue;
            }
            let chunk = &self.bytes[at..at + size];
            let _ = self.in_package(chunk, kind, entry, &mut out);
        }
        out
    }

    fn in_package(
        &self,
        pkg: &[u8],
        kind: u8,
        entry: u32,
        out: &mut Vec<Found>,
    ) -> Result<(), ArscError> {
        let mut at = u16_at(pkg, 2)? as usize;
        while at + 8 <= pkg.len() {
            let ckind = u16_at(pkg, at)?;
            let size = u32_at(pkg, at + 4)? as usize;
            if size < 8 || at + size > pkg.len() {
                return Err(ArscError);
            }
            let chunk = &pkg[at..at + size];
            if ckind == RES_TABLE_TYPE && chunk.get(8) == Some(&kind) {
                if let Some(found) = self.in_type(chunk, entry)? {
                    out.push(found);
                }
            }
            at += size;
        }
        Ok(())
    }

    /// Offset (from `entries_start`) of entry `index` in a type chunk, if it has one.
    fn offset_of(
        chunk: &[u8],
        header: usize,
        flags: u8,
        count: usize,
        index: u32,
    ) -> Result<Option<usize>, ArscError> {
        if flags & TYPE_SPARSE != 0 {
            for i in 0..count {
                let idx = u16_at(chunk, header + 4 * i)?;
                if u32::from(idx) == index {
                    return Ok(Some(4 * u16_at(chunk, header + 4 * i + 2)? as usize));
                }
            }
            return Ok(None);
        }
        if index as usize >= count {
            return Ok(None);
        }
        if flags & TYPE_OFFSET16 != 0 {
            let o = u16_at(chunk, header + 2 * index as usize)?;
            return Ok((o != 0xffff).then_some(4 * o as usize));
        }
        let o = u32_at(chunk, header + 4 * index as usize)?;
        Ok((o != NO_ENTRY).then_some(o as usize))
    }

    fn in_type(&self, chunk: &[u8], index: u32) -> Result<Option<Found>, ArscError> {
        let header = u16_at(chunk, 2)? as usize;
        let flags = *chunk.get(9).ok_or(ArscError)?;
        let count = u32_at(chunk, 12)? as usize;
        let start = u32_at(chunk, 16)? as usize;
        if count > MAX_ENTRIES {
            return Err(ArscError);
        }
        // ResTable_config at 20: size, mcc, mnc, language[2], country[2], …, density at +14
        let config = 20;
        let lang = chunk.get(config + 8..config + 10).ok_or(ArscError)?;
        let language = if lang[0] == 0 || lang[0] & 0x80 != 0 {
            String::new()
        } else {
            String::from_utf8_lossy(lang).into_owned()
        };
        let density = u16_at(chunk, config + 14)?;
        let Some(offset) = Self::offset_of(chunk, header, flags, count, index)? else {
            return Ok(None);
        };
        let at = start.checked_add(offset).ok_or(ArscError)?;
        let eflags = u16_at(chunk, at + 2)?;
        let value = if eflags & FLAG_COMPACT != 0 {
            let kind = (eflags >> 8) as u8;
            self.value(kind, u32_at(chunk, at + 4)?)
        } else if eflags & FLAG_COMPLEX != 0 {
            Resolved::Other
        } else {
            let size = u16_at(chunk, at)? as usize;
            let v = at + size; // Res_value: size u16, res0 u8, dataType u8, data u32
            self.value(*chunk.get(v + 3).ok_or(ArscError)?, u32_at(chunk, v + 4)?)
        };
        Ok(Some(Found {
            language,
            density,
            value,
        }))
    }

    fn value(&self, kind: u8, data: u32) -> Resolved {
        match kind {
            0x03 => self
                .pool
                .get(data)
                .map_or(Resolved::Other, |s| Resolved::Text(s.to_owned())),
            0x01 => Resolved::Ref(data),
            _ => Resolved::Other,
        }
    }

    /// The strings of `id` by language (references followed a few times).
    pub fn strings(&self, id: u32) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        self.collect(id, 4, &mut |f| {
            if let Resolved::Text(s) = &f.value {
                if !out.iter().any(|(l, _)| *l == f.language) {
                    out.push((f.language.clone(), s.clone()));
                }
            }
        });
        out
    }

    /// The file paths of `id` with their density (references followed a few times).
    pub fn files(&self, id: u32) -> Vec<(u16, String)> {
        let mut out = Vec::new();
        self.collect(id, 4, &mut |f| {
            if let Resolved::Text(s) = &f.value {
                out.push((f.density, s.clone()));
            }
        });
        out
    }

    fn collect(&self, id: u32, depth: u8, sink: &mut dyn FnMut(&Found)) {
        for f in self.lookup(id) {
            match f.value {
                Resolved::Ref(next) if depth > 0 && next != id => {
                    self.collect(next, depth - 1, sink)
                }
                _ => sink(&f),
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::apk::axml::tests::pool_chunk;

    /// A table with package 0x7f: type 1 entry 0 = string `label` (default) and
    /// `label_ar` (ar); type 2 entry 0 = the icon paths of two densities.
    pub fn sample(label: &str, label_ar: &str, icons: &[(u16, &str)]) -> Vec<u8> {
        let mut strings = vec![label.to_owned(), label_ar.to_owned()];
        strings.extend(icons.iter().map(|(_, p)| (*p).to_owned()));
        let pool = pool_chunk(&strings);
        let empty = pool_chunk(&[]);
        let type_chunk = |kind: u8, lang: &[u8; 2], density: u16, string: u32| {
            let header = 20 + 64; // fixed part + config
            let mut c = Vec::new();
            c.extend_from_slice(&RES_TABLE_TYPE.to_le_bytes());
            c.extend_from_slice(&(header as u16).to_le_bytes());
            let size = header + 4 + 16;
            c.extend_from_slice(&(size as u32).to_le_bytes());
            c.push(kind);
            c.push(0);
            c.extend_from_slice(&[0, 0]);
            c.extend_from_slice(&1u32.to_le_bytes());
            c.extend_from_slice(&((header + 4) as u32).to_le_bytes());
            let mut config = vec![0u8; 64];
            config[..4].copy_from_slice(&64u32.to_le_bytes());
            config[8..10].copy_from_slice(lang);
            config[14..16].copy_from_slice(&density.to_le_bytes());
            c.extend_from_slice(&config);
            c.extend_from_slice(&0u32.to_le_bytes()); // offset of entry 0
            c.extend_from_slice(&8u16.to_le_bytes()); // entry size
            c.extend_from_slice(&0u16.to_le_bytes()); // flags
            c.extend_from_slice(&0u32.to_le_bytes()); // key
            c.extend_from_slice(&8u16.to_le_bytes());
            c.push(0);
            c.push(0x03);
            c.extend_from_slice(&string.to_le_bytes());
            c
        };
        let mut body = Vec::new();
        body.extend(type_chunk(1, &[0, 0], 0, 0));
        body.extend(type_chunk(1, b"ar", 0, 1));
        for (i, (d, _)) in icons.iter().enumerate() {
            body.extend(type_chunk(2, &[0, 0], *d, 2 + i as u32));
        }
        let pkg_header = 288;
        let mut pkg = Vec::new();
        pkg.extend_from_slice(&RES_TABLE_PACKAGE.to_le_bytes());
        pkg.extend_from_slice(&(pkg_header as u16).to_le_bytes());
        let pkg_size = pkg_header + 2 * empty.len() + body.len();
        pkg.extend_from_slice(&(pkg_size as u32).to_le_bytes());
        pkg.extend_from_slice(&0x7fu32.to_le_bytes());
        pkg.resize(pkg_header, 0);
        pkg.extend_from_slice(&empty);
        pkg.extend_from_slice(&empty);
        pkg.extend_from_slice(&body);
        let mut out = Vec::new();
        out.extend_from_slice(&RES_TABLE.to_le_bytes());
        out.extend_from_slice(&12u16.to_le_bytes());
        out.extend_from_slice(&((12 + pool.len() + pkg.len()) as u32).to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&pool);
        out.extend_from_slice(&pkg);
        out
    }

    #[test]
    fn names_by_language_and_icons_by_density_resolve() {
        let bytes = sample(
            "Calculator",
            "آلة حاسبة",
            &[
                (240, "res/mipmap-hdpi/ic.png"),
                (640, "res/mipmap-xxxhdpi/ic.png"),
            ],
        );
        let t = Table::parse(&bytes).unwrap();
        let names = t.strings(0x7f01_0000);
        assert!(names.contains(&(String::new(), "Calculator".into())));
        assert!(names.contains(&("ar".into(), "آلة حاسبة".into())));
        let files = t.files(0x7f02_0000);
        assert!(files.contains(&(640, "res/mipmap-xxxhdpi/ic.png".into())));
        assert_eq!(files.len(), 2);
        assert!(t.strings(0x7f05_0000).is_empty());
        assert!(t.strings(0x0101_0000).is_empty());
    }

    #[test]
    fn damaged_tables_never_panic() {
        let good = sample("A", "B", &[(160, "res/x.png")]);
        let mut seed = 0xdead_beef_u64;
        for _ in 0..5000 {
            let mut copy = good.clone();
            for _ in 0..3 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let at = (seed as usize) % copy.len();
                copy[at] = (seed >> 24) as u8;
            }
            if let Ok(t) = Table::parse(&copy) {
                let _ = t.strings(0x7f01_0000);
                let _ = t.files(0x7f02_0000);
            }
        }
    }
}
