//! What a Windows program says about itself – read from its headers and its version
//! resource, never executed. Product name, version and company are the file's own
//! words (unverified); the installer kind decides how the program is installed.
//!
//! Every offset, size and count is checked before it is used; nothing is read past
//! [`MAX_RESOURCES`] bytes of resources or [`MAX_OVERLAY_SCAN`] bytes after the image,
//! and a damaged file is "not a program", never a panic (`tests::fuzz`).

use crate::winread::{utf16_z, ReadAt};

/// Largest resource section that is read.
const MAX_RESOURCES: u64 = 16 << 20;
/// Bytes after the image searched for an installer's mark.
const MAX_OVERLAY_SCAN: u64 = 4 << 20;
/// Most sections of an image.
const MAX_SECTIONS: usize = 96;
/// Most entries of one resource directory.
const MAX_ENTRIES: usize = 1024;
/// Largest version resource.
const MAX_VERSION: usize = 64 << 10;
/// Longest text taken from the file.
pub const MAX_TEXT: usize = 128;
/// Largest icon taken from the file.
pub const MAX_ICON: usize = 1 << 20;

/// Which program installs this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installer {
    /// Nullsoft (NSIS): silent with `/S`.
    Nsis,
    /// Inno Setup: silent with `/VERYSILENT`.
    Inno,
    /// Not an installer this service knows: the program is the app itself.
    Portable,
}

/// What the file says (all of it unverified).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeInfo {
    /// `ProductName` (or `FileDescription`).
    pub product: String,
    /// `ProductVersion` (or the fixed version).
    pub version: String,
    /// `CompanyName`.
    pub company: String,
    /// 64-bit image.
    pub wide: bool,
    /// A library, not a program (never installed).
    pub dll: bool,
    /// Kind of installer.
    pub installer: Option<Installer>,
}

#[derive(Clone, Copy)]
struct Section {
    va: u32,
    vsize: u32,
    raw_size: u32,
    raw_at: u32,
}

/// The parts of an image this module reads.
struct Image {
    sections: Vec<Section>,
    /// The resource directory's bytes and their RVA.
    rsrc: Option<(Vec<u8>, u32)>,
    wide: bool,
    dll: bool,
    end_of_image: u64,
}

impl Image {
    fn open(file: &dyn ReadAt) -> Option<Self> {
        let at = u64::from(file.u32_at(0x3c)?);
        if at > 64 << 10 || file.bytes(0, 2)? != b"MZ" || file.bytes(at, 4)? != b"PE\0\0" {
            return None;
        }
        let coff = at + 4;
        let count = usize::from(file.u16_at(coff + 2)?);
        let optional = usize::from(file.u16_at(coff + 16)?);
        let dll = file.u16_at(coff + 18)? & 0x2000 != 0;
        let opt = coff + 20;
        let wide = match file.u16_at(opt)? {
            0x10b => false,
            0x20b => true,
            _ => return None,
        };
        let (dirs_at, ndirs_at) = if wide { (112, 108) } else { (96, 92) };
        if count == 0 || count > MAX_SECTIONS || optional < dirs_at + 24 {
            return None;
        }
        let ndirs = file.u32_at(opt + ndirs_at)?;
        let sections = sections(file, opt + optional as u64, count)?;
        let end_of_image = sections
            .iter()
            .map(|s| u64::from(s.raw_at) + u64::from(s.raw_size))
            .max()
            .unwrap_or(0)
            .min(file.len());
        let rsrc = if ndirs > 2 {
            let rva = file.u32_at(opt + dirs_at as u64 + 16)?;
            let size = file.u32_at(opt + dirs_at as u64 + 20)?;
            offset_of(&sections, rva).and_then(|(at, room)| {
                let take = u64::from(size).min(room).min(MAX_RESOURCES);
                Some((file.bytes(at, usize::try_from(take).ok()?)?, rva))
            })
        } else {
            None
        };
        Some(Self {
            sections,
            rsrc,
            wide,
            dll,
            end_of_image,
        })
    }

    /// The data of every resource of `kind` (first language of each name), at most `max`
    /// bytes each and 256 of them.
    fn resources(&self, file: &dyn ReadAt, kind: u32, max: usize) -> Vec<Vec<u8>> {
        let Some((rsrc, _)) = &self.rsrc else {
            return Vec::new();
        };
        let Some(names) = find_entry(rsrc, 0, Some(kind)).filter(|e| e & 0x8000_0000 != 0) else {
            return Vec::new();
        };
        entries(rsrc, (names & 0x7fff_ffff) as usize)
            .into_iter()
            .filter(|e| e & 0x8000_0000 != 0)
            .take(256)
            .filter_map(|lang_dir| {
                let leaf = find_entry(rsrc, (lang_dir & 0x7fff_ffff) as usize, None)?;
                if leaf & 0x8000_0000 != 0 {
                    return None;
                }
                let at = (leaf & 0x7fff_ffff) as usize;
                let (rva, size) = (le32(rsrc, at)?, le32(rsrc, at + 4)? as usize);
                if size > max {
                    return None;
                }
                let (data_at, room) = offset_of(&self.sections, rva)?;
                file.bytes(data_at, size.min(usize::try_from(room).ok()?))
            })
            .collect()
    }
}

/// Reads `file` as a PE image; `None`: not one (or too damaged to say anything).
pub fn read(file: &dyn ReadAt) -> Option<PeInfo> {
    let image = Image::open(file)?;
    let mut info = PeInfo {
        wide: image.wide,
        dll: image.dll,
        ..PeInfo::default()
    };
    if let Some(block) = image.resources(file, 16, MAX_VERSION).into_iter().next() {
        fill(&mut info, &block);
    }
    info.installer = Some(installer_of(file, image.end_of_image, &info));
    Some(info)
}

/// The largest PNG among the program's icons (modern programs keep their big icon as
/// PNG), at most [`MAX_ICON`] bytes.
pub fn png_icon(file: &dyn ReadAt) -> Option<Vec<u8>> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    Image::open(file)?
        .resources(file, 3, MAX_ICON)
        .into_iter()
        .filter(|d| d.starts_with(PNG))
        .max_by_key(Vec::len)
}

fn sections(file: &dyn ReadAt, table: u64, count: usize) -> Option<Vec<Section>> {
    (0..count)
        .map(|i| {
            let s = table + 40 * i as u64;
            Some(Section {
                vsize: file.u32_at(s + 8)?,
                va: file.u32_at(s + 12)?,
                raw_size: file.u32_at(s + 16)?,
                raw_at: file.u32_at(s + 20)?,
            })
        })
        .collect()
}

/// The file offset of `rva`, and how many bytes of its section follow it in the file.
fn offset_of(sections: &[Section], rva: u32) -> Option<(u64, u64)> {
    sections.iter().find_map(|s| {
        let span = s.vsize.max(s.raw_size);
        let inside = rva >= s.va && rva - s.va < span;
        let into = rva.checked_sub(s.va)?;
        (inside && into < s.raw_size).then(|| {
            (
                u64::from(s.raw_at) + u64::from(into),
                u64::from(s.raw_size - into),
            )
        })
    })
}

/// `(name, target)` of the entries of the directory at `dir`.
fn named_entries(rsrc: &[u8], dir: usize) -> Vec<(u32, u32)> {
    let total = le16(rsrc, dir + 12)
        .zip(le16(rsrc, dir + 14))
        .map_or(0, |(n, i)| usize::from(n) + usize::from(i));
    if total > MAX_ENTRIES {
        return Vec::new();
    }
    (0..total)
        .map_while(|i| {
            let e = dir + 16 + 8 * i;
            Some((le32(rsrc, e)?, le32(rsrc, e + 4)?))
        })
        .collect()
}

/// The targets of the entries of the directory at `dir`.
fn entries(rsrc: &[u8], dir: usize) -> Vec<u32> {
    named_entries(rsrc, dir)
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

/// The entry `wanted` (an id) of the directory at `dir`, or its first entry.
fn find_entry(rsrc: &[u8], dir: usize, wanted: Option<u32>) -> Option<u32> {
    named_entries(rsrc, dir)
        .into_iter()
        .find(|(name, _)| wanted.is_none_or(|id| *name == id))
        .map(|(_, target)| target)
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// One node of a version resource: `(key, value bytes, children)`.
struct Node<'a> {
    key: String,
    value: &'a [u8],
    text: bool,
    children: &'a [u8],
}

fn node(b: &[u8]) -> Option<(Node<'_>, usize)> {
    let length = usize::from(le16(b, 0)?);
    let value_len = usize::from(le16(b, 2)?);
    let text = le16(b, 4)? == 1;
    if length < 6 || length > b.len() {
        return None;
    }
    let block = &b[..length];
    let (key, key_end) = utf16_z(block, 6, 64)?;
    let value_at = align4(key_end);
    let value_bytes = if text { value_len * 2 } else { value_len };
    let value_end = value_at.checked_add(value_bytes)?.min(length);
    let value = block.get(value_at.min(length)..value_end)?;
    let children = block.get(align4(value_end).min(length)..)?;
    Some((
        Node {
            key,
            value,
            text,
            children,
        },
        align4(length),
    ))
}

const fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// The child nodes of a block, at most 256.
fn nodes(mut b: &[u8]) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    while out.len() < 256 {
        let Some((n, step)) = node(b) else {
            break;
        };
        out.push(n);
        let Some(rest) = b.get(step..) else {
            break;
        };
        b = rest;
    }
    out
}

fn fill(info: &mut PeInfo, block: &[u8]) {
    let Some((root, _)) = node(block) else {
        return;
    };
    if root.key != "VS_VERSION_INFO" {
        return;
    }
    let mut strings: Vec<(String, String)> = Vec::new();
    for child in nodes(root.children) {
        if child.key != "StringFileInfo" {
            continue;
        }
        // the first string table (the file's first language)
        if let Some(table) = nodes(child.children).into_iter().next() {
            for s in nodes(table.children).into_iter().filter(|s| s.text) {
                let words = s.value.len() / 2;
                if let Some((v, _)) = utf16_z(s.value, 0, words.min(1024)) {
                    strings.push((s.key, clean(&v)));
                }
            }
        }
    }
    let get = |k: &str| {
        strings
            .iter()
            .find(|(key, v)| key == k && !v.is_empty())
            .map(|(_, v)| v.clone())
    };
    info.product = get("ProductName")
        .or_else(|| get("FileDescription"))
        .unwrap_or_default();
    info.company = get("CompanyName").unwrap_or_default();
    info.version = get("ProductVersion")
        .or_else(|| get("FileVersion"))
        .or_else(|| fixed_version(root.value))
        .unwrap_or_default();
    if strings
        .iter()
        .any(|(k, v)| k == "Comments" && v.contains("Inno Setup"))
    {
        info.installer = Some(Installer::Inno);
    }
}

/// `a.b.c.d` from `VS_FIXEDFILEINFO` (product version).
fn fixed_version(v: &[u8]) -> Option<String> {
    if le32(v, 0)? != 0xFEEF_04BD {
        return None;
    }
    let (ms, ls) = (le32(v, 16)?, le32(v, 20)?);
    Some(format!(
        "{}.{}.{}.{}",
        ms >> 16,
        ms & 0xffff,
        ls >> 16,
        ls & 0xffff
    ))
}

/// Text from the file: no control characters, trimmed, at most [`MAX_TEXT`] characters.
pub fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// NSIS marks the data after its image with `0xDEADBEEF "NullsoftInst"`; Inno Setup says so
/// in its version resource.
fn installer_of(file: &dyn ReadAt, end_of_image: u64, info: &PeInfo) -> Installer {
    if info.installer == Some(Installer::Inno) {
        return Installer::Inno;
    }
    const MARK: &[u8] = b"\xEF\xBE\xAD\xDENullsoftInst";
    let mut at = end_of_image;
    let stop = end_of_image
        .saturating_add(MAX_OVERLAY_SCAN)
        .min(file.len());
    while at < stop {
        let n = (stop - at).min(256 << 10);
        let Some(chunk) = file.bytes(at, n as usize) else {
            break;
        };
        if chunk.windows(MARK.len()).any(|w| w == MARK) {
            return Installer::Nsis;
        }
        // overlap so that a mark across two chunks is found
        at += n.saturating_sub(MARK.len() as u64).max(1);
    }
    Installer::Portable
}

#[cfg(test)]
pub(crate) mod tests;
