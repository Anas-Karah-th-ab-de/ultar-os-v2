//! The time zone's offset from UTC (feature `std`): from the zone file (TZif,
//! `/usr/share/zoneinfo/<name>`, also in the Ubuntu area `/compat/…`) when the system has
//! one, otherwise from a short table: zones without summer time (the Arab world's and UTC)
//! and the zones with summer time the installer offers (Beirut, Berlin), by their rules.
//! The setting is `datetime.timezone.name` (default `Asia/Damascus`, like settingsd). The
//! desktop, the lock screen and the phone shell all take their clock's offset from here.

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

/// Zones with summer time from the last Sunday of March to the last Sunday of October:
/// name, standard offset, summer offset (seconds east of UTC), and the hour of the change
/// in standard local time (Berlin: 2:00 → 3:00 / 3:00 → 2:00 summer; Beirut: 0:00).
const RULED: [(&str, i32, i32, i64); 7] = [
    ("Europe/Berlin", 3600, 7200, 2),
    ("Europe/Paris", 3600, 7200, 2),
    ("Europe/Vienna", 3600, 7200, 2),
    ("Europe/Amsterdam", 3600, 7200, 2),
    ("Europe/London", 0, 3600, 1),
    ("Asia/Beirut", 7200, 10800, 0),
    ("Asia/Nicosia", 7200, 10800, 3),
];

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year of a day since 1970-01-01.
fn year_of(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

/// Day (since 1970-01-01) of the last Sunday of `month` in `year`.
fn last_sunday(year: i64, month: i64) -> i64 {
    let next = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    let last = next - 1;
    // 1970-01-01 was a Thursday: weekday 0 = Sunday at day 3.
    let weekday = (last + 4).rem_euclid(7);
    last - weekday
}

/// The offset of a ruled zone at `unix`.
fn ruled(name: &str, unix: i64) -> Option<i32> {
    let (_, standard, summer, hour) = RULED.iter().find(|(n, ..)| *n == name)?;
    let year = year_of((unix + i64::from(*standard)).div_euclid(86_400));
    let start = last_sunday(year, 3) * 86_400 + hour * 3600 - i64::from(*standard);
    let end = last_sunday(year, 10) * 86_400 + (hour + 1) * 3600 - i64::from(*summer);
    Some(if (start..end).contains(&unix) {
        *summer
    } else {
        *standard
    })
}

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
    if let Some(o) = ruled(name, unix) {
        return o;
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
    fn summer_time_follows_the_rules_without_a_zone_file() {
        // 2026: summer time in Europe from 29 March 01:00 UTC to 25 October 01:00 UTC.
        let utc = |y: i64, m: i64, d: i64, h: i64| days_from_civil(y, m, d) * 86_400 + h * 3600;
        assert_eq!(ruled("Europe/Berlin", utc(2026, 1, 15, 12)), Some(3600));
        assert_eq!(ruled("Europe/Berlin", utc(2026, 3, 29, 0)), Some(3600));
        assert_eq!(ruled("Europe/Berlin", utc(2026, 3, 29, 1)), Some(7200));
        assert_eq!(ruled("Europe/Berlin", utc(2026, 7, 1, 0)), Some(7200));
        assert_eq!(ruled("Europe/Berlin", utc(2026, 10, 25, 0)), Some(7200));
        assert_eq!(ruled("Europe/Berlin", utc(2026, 10, 25, 1)), Some(3600));
        assert_eq!(ruled("Asia/Beirut", utc(2026, 7, 1, 0)), Some(10800));
        assert_eq!(ruled("Asia/Beirut", utc(2026, 12, 1, 0)), Some(7200));
        assert_eq!(ruled("Asia/Damascus", 0), None);
        assert_eq!(last_sunday(2026, 3), days_from_civil(2026, 3, 29));
        assert_eq!(year_of(days_from_civil(2026, 12, 31)), 2026);
        assert_eq!(year_of(days_from_civil(2024, 2, 29)), 2024);
    }

    #[test]
    fn the_system_zone_files_agree_where_they_exist() {
        // Damascus has been UTC+3 the whole year since 2022.
        let o = offset("Asia/Damascus", 1_795_636_560);
        assert_eq!(o, 10800);
    }
}
