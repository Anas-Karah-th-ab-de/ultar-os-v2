//! The time zone's offset from UTC: from the zone file (TZif, `/usr/share/zoneinfo/<name>`,
//! also in the Ubuntu area `/compat/…`) when the system has one, otherwise from a short
//! table of zones without summer time (the Arab world's and UTC). The setting is
//! `datetime.timezone.name` (default `Asia/Damascus`, like settingsd).

use std::path::Path;

/// The zone when nothing is set (settingsd's default).
pub const DEFAULT: &str = "Asia/Damascus";
/// Where zone files are looked for.
const DIRS: [&str; 2] = ["/usr/share/zoneinfo", "/compat/usr/share/zoneinfo"];
/// Zones without summer time: name → seconds east of UTC.
const FIXED: [(&str, i32); 14] = [
    ("UTC", 0),
    ("Etc/UTC", 0),
    ("Asia/Damascus", 3 * 3600),
    ("Asia/Amman", 3 * 3600),
    ("Asia/Baghdad", 3 * 3600),
    ("Asia/Riyadh", 3 * 3600),
    ("Asia/Kuwait", 3 * 3600),
    ("Asia/Qatar", 3 * 3600),
    ("Asia/Bahrain", 3 * 3600),
    ("Europe/Istanbul", 3 * 3600),
    ("Asia/Dubai", 4 * 3600),
    ("Asia/Muscat", 4 * 3600),
    ("Africa/Khartoum", 2 * 3600),
    ("Africa/Tripoli", 2 * 3600),
];

/// A zone name as settingsd allows it (`Area/City`, letters, `_`, `-`, `+`, digits).
pub fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('/')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
}

/// Seconds east of UTC for `name` at `unix` seconds.
pub fn offset(name: &str, unix: i64) -> i32 {
    if valid(name) {
        for dir in DIRS {
            if let Ok(data) = std::fs::read(Path::new(dir).join(name)) {
                if let Some(o) = tzif_offset(&data, unix) {
                    return o;
                }
            }
        }
    }
    FIXED
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(0, |(_, o)| *o)
}

fn be32(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The offset in a TZif file at `unix` (version 2+ data block preferred, else version 1).
pub fn tzif_offset(data: &[u8], unix: i64) -> Option<i32> {
    if data.get(..4)? != b"TZif" {
        return None;
    }
    let counts = |at: usize| -> Option<[usize; 6]> {
        let mut c = [0usize; 6];
        for (i, slot) in c.iter_mut().enumerate() {
            *slot = usize::try_from(be32(data, at + 20 + i * 4)?).ok()?;
        }
        Some(c)
    };
    let v1 = counts(0)?;
    let v1_len = v1[3] * 5 + v1[4] * 6 + v1[5] + v1[2] * 8 + v1[1] + v1[0];
    if data.get(4).copied().unwrap_or(0) >= b'2' {
        let start = 44 + v1_len;
        if data.get(start..start + 4) == Some(b"TZif") {
            let c = counts(start)?;
            return block(data, start + 44, c, 8, unix);
        }
    }
    block(data, 44, v1, 4, unix)
}

/// One data block: transitions (`width` bytes each), their types, the types.
fn block(data: &[u8], at: usize, c: [usize; 6], width: usize, unix: i64) -> Option<i32> {
    let (timecnt, typecnt) = (c[3], c[4]);
    if typecnt == 0 {
        return None;
    }
    let times = at;
    let idx = times + timecnt * width;
    let types = idx + timecnt;
    let mut chosen = 0usize;
    for i in 0..timecnt {
        let t = if width == 8 {
            be64(data, times + i * 8)?
        } else {
            i64::from(be32(data, times + i * 4)?)
        };
        if t > unix {
            break;
        }
        chosen = usize::from(*data.get(idx + i)?);
    }
    be32(data, types + chosen.min(typecnt - 1) * 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version-1 TZif with two types (+2 h, +3 h) and one transition at 1000.
    fn sample() -> Vec<u8> {
        let mut b = b"TZif".to_vec();
        b.push(0);
        b.extend([0u8; 15]);
        for n in [0u32, 0, 0, 1, 2, 0] {
            b.extend(n.to_be_bytes());
        }
        b.extend(1000i32.to_be_bytes());
        b.push(1);
        b.extend(7200i32.to_be_bytes());
        b.extend([0, 0]);
        b.extend(10800i32.to_be_bytes());
        b.extend([0, 0]);
        b
    }

    #[test]
    fn a_zone_file_gives_the_offset_of_the_moment() {
        let data = sample();
        assert_eq!(tzif_offset(&data, 500), Some(7200));
        assert_eq!(tzif_offset(&data, 2000), Some(10800));
        assert_eq!(tzif_offset(b"nope", 0), None);
    }

    #[test]
    fn known_zones_without_a_file_and_bad_names() {
        assert_eq!(
            FIXED
                .iter()
                .find(|(n, _)| *n == "Asia/Damascus")
                .map(|x| x.1),
            Some(10800)
        );
        assert!(valid("Asia/Damascus") && valid("Etc/GMT+3"));
        assert!(!valid("../etc/passwd") && !valid("/etc/x") && !valid(""));
        assert_eq!(offset("../../etc/shadow", 0), 0);
    }

    #[test]
    fn the_system_zone_files_agree_where_they_exist() {
        // Damascus has been UTC+3 the whole year since 2022.
        let o = offset("Asia/Damascus", 1_795_636_560);
        assert_eq!(o, 10800);
    }
}
