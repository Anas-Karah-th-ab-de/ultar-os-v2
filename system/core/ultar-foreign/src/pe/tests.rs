//! Tests of the reading of Windows programs.

use super::*;

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn pad(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

/// A version node (`text`: a string value).
fn vnode(key: &str, value: &[u8], text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0, 0];
    let value_len = if text { value.len() / 2 } else { value.len() };
    put16(&mut b, value_len as u16);
    put16(&mut b, u16::from(text));
    b.extend(utf16(key));
    pad(&mut b);
    b.extend_from_slice(value);
    pad(&mut b);
    for c in children {
        b.extend_from_slice(c);
        pad(&mut b);
    }
    let len = b.len() as u16;
    b[..2].copy_from_slice(&len.to_le_bytes());
    b
}

/// A small PE32+ with a version resource and, optionally, data after its image.
/// A resource section at RVA 0x1000: one name (1) and one language (0x409) per kind.
fn resource_tree(kinds: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let n = kinds.len();
    // root dir, then per kind: name dir, lang dir, data entry
    let dir_size = |entries: usize| 16 + 8 * entries;
    let mut r = vec![0u8; dir_size(n)];
    r[14..16].copy_from_slice(&(n as u16).to_le_bytes());
    let mut leaves = Vec::new();
    for (i, (kind, _)) in kinds.iter().enumerate() {
        let name_dir = r.len();
        r.resize(name_dir + dir_size(1) * 2 + 16, 0);
        let lang_dir = name_dir + dir_size(1);
        let leaf = lang_dir + dir_size(1);
        let e = 16 + 8 * i;
        r[e..e + 4].copy_from_slice(&kind.to_le_bytes());
        r[e + 4..e + 8].copy_from_slice(&(0x8000_0000 | name_dir as u32).to_le_bytes());
        r[name_dir + 14..name_dir + 16].copy_from_slice(&1u16.to_le_bytes());
        r[name_dir + 16..name_dir + 20].copy_from_slice(&1u32.to_le_bytes());
        r[name_dir + 20..name_dir + 24]
            .copy_from_slice(&(0x8000_0000 | lang_dir as u32).to_le_bytes());
        r[lang_dir + 14..lang_dir + 16].copy_from_slice(&1u16.to_le_bytes());
        r[lang_dir + 16..lang_dir + 20].copy_from_slice(&0x409u32.to_le_bytes());
        r[lang_dir + 20..lang_dir + 24].copy_from_slice(&(leaf as u32).to_le_bytes());
        leaves.push(leaf);
    }
    for ((_, data), leaf) in kinds.iter().zip(leaves) {
        let rva = 0x1000 + r.len() as u32;
        r[leaf..leaf + 4].copy_from_slice(&rva.to_le_bytes());
        r[leaf + 4..leaf + 8].copy_from_slice(&(data.len() as u32).to_le_bytes());
        r.extend_from_slice(data);
        pad(&mut r);
    }
    while r.len() % 0x200 != 0 {
        r.push(0);
    }
    r
}

pub(crate) fn sample(strings: &[(&str, &str)], overlay: &[u8]) -> Vec<u8> {
    sample_with_icon(strings, overlay, None)
}

pub(crate) fn sample_with_icon(
    strings: &[(&str, &str)],
    overlay: &[u8],
    icon: Option<&[u8]>,
) -> Vec<u8> {
    let table: Vec<Vec<u8>> = strings
        .iter()
        .map(|(k, v)| vnode(k, &utf16(v), true, &[]))
        .collect();
    let sfi = vnode(
        "StringFileInfo",
        &[],
        true,
        &[vnode("040904b0", &[], true, &table)],
    );
    let mut fixed = vec![0u8; 52];
    fixed[..4].copy_from_slice(&0xFEEF_04BDu32.to_le_bytes());
    fixed[16..20].copy_from_slice(&((1u32 << 16) | 2).to_le_bytes());
    fixed[20..24].copy_from_slice(&((3u32 << 16) | 4).to_le_bytes());
    let version = vnode("VS_VERSION_INFO", &fixed, false, &[sfi]);
    let mut kinds: Vec<(u32, Vec<u8>)> = vec![(16, version)];
    if let Some(icon) = icon {
        kinds.insert(0, (3, icon.to_vec()));
    }
    let rsrc = resource_tree(&kinds);
    let mut f = vec![0u8; 0x400];
    f[..2].copy_from_slice(b"MZ");
    f[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    f[0x80..0x84].copy_from_slice(b"PE\0\0");
    let coff = 0x84;
    f[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
    f[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
    f[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
    let opt = coff + 20;
    f[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
    f[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
    f[opt + 112 + 16..opt + 112 + 20].copy_from_slice(&0x1000u32.to_le_bytes());
    f[opt + 112 + 20..opt + 112 + 24].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
    let sec = opt + 240;
    f[sec..sec + 5].copy_from_slice(b".rsrc");
    f[sec + 8..sec + 12].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
    f[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
    f[sec + 16..sec + 20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
    f[sec + 20..sec + 24].copy_from_slice(&0x400u32.to_le_bytes());
    f.extend_from_slice(&rsrc);
    f.extend_from_slice(overlay);
    f
}

#[test]
fn the_version_resource_is_read() {
    let pe = sample(
        &[
            ("CompanyName", "Don Ho"),
            ("ProductName", "Notepad++"),
            ("ProductVersion", "8.7.1"),
        ],
        &[],
    );
    let info = read(&pe.as_slice()).unwrap();
    assert_eq!(info.product, "Notepad++");
    assert_eq!(info.company, "Don Ho");
    assert_eq!(info.version, "8.7.1");
    assert!(info.wide);
    assert_eq!(info.installer, Some(Installer::Portable));
}

#[test]
fn installers_are_recognised() {
    let mut overlay = vec![0u8; 700];
    overlay.extend_from_slice(b"\x00\x00\x00\x00\xEF\xBE\xAD\xDENullsoftInst");
    let nsis = sample(&[("ProductName", "X")], &overlay);
    assert_eq!(
        read(&nsis.as_slice()).unwrap().installer,
        Some(Installer::Nsis)
    );
    let inno = sample(
        &[("Comments", "This installation was built with Inno Setup.")],
        &[],
    );
    let i = read(&inno.as_slice()).unwrap();
    assert_eq!(i.installer, Some(Installer::Inno));
    // no product name: the fixed version and nothing else
    assert_eq!(i.product, "");
    assert_eq!(i.version, "1.2.3.4");
}

#[test]
fn texts_are_cleaned_and_bounded() {
    let long = "A\u{7}".repeat(400);
    let pe = sample(
        &[("ProductName", &long), ("CompanyName", "  Evil\n Corp ")],
        &[],
    );
    let info = read(&pe.as_slice()).unwrap();
    assert_eq!(info.product.chars().count(), MAX_TEXT);
    assert!(!info.product.contains('\u{7}'));
    assert_eq!(info.company, "Evil Corp");
}

#[test]
fn the_largest_png_icon_is_taken() {
    let png = b"\x89PNG\r\n\x1a\n-a-tiny-picture";
    let pe = sample_with_icon(&[("ProductName", "P")], &[], Some(png));
    assert_eq!(png_icon(&pe.as_slice()), Some(png.to_vec()));
    assert_eq!(read(&pe.as_slice()).unwrap().product, "P");
    let plain = sample(&[("ProductName", "P")], &[]);
    assert_eq!(png_icon(&plain.as_slice()), None);
    let bmp = sample_with_icon(&[], &[], Some(b"\x28\0\0\0 a bitmap icon"));
    assert_eq!(png_icon(&bmp.as_slice()), None);
}

#[test]
fn not_programs() {
    for bad in [&b""[..], b"MZ", b"MZ\0\0\0\0", b"PK\x03\x04"] {
        assert!(read(&bad).is_none());
    }
    let mut no_sig = sample(&[], &[]);
    no_sig[0x80] = b'X';
    assert!(read(&no_sig.as_slice()).is_none());
}

/// Fixed-seed random damage of a valid program: never a panic, never a hang.
#[test]
fn fuzz() {
    let mut overlay = vec![7u8; 300];
    overlay.extend_from_slice(b"\xEF\xBE\xAD\xDENullsoftInst");
    let base = sample_with_icon(
        &[("ProductName", "P"), ("CompanyName", "C")],
        &overlay,
        Some(b"\x89PNG\r\n\x1a\nicon"),
    );
    let mut rng = crate::winread::Rng(0x5eed_1234);
    for _ in 0..4000 {
        let mut f = base.clone();
        for _ in 0..1 + rng.below(16) {
            let at = rng.below(f.len());
            f[at] = rng.step() as u8;
        }
        if rng.below(8) == 0 {
            f.truncate(rng.below(f.len()));
        }
        if let Some(info) = read(&f.as_slice()) {
            assert!(info.product.chars().count() <= MAX_TEXT);
            assert!(info.company.chars().count() <= MAX_TEXT);
        }
        if let Some(icon) = png_icon(&f.as_slice()) {
            assert!(icon.len() <= MAX_ICON);
        }
    }
}
