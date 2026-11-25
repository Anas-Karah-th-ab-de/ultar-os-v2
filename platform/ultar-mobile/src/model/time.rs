//! The time the phone shows: a moment in local time and the lines built from it. Digits are
//! always 0–9.

use ultar_i18n::calendar::Date;
use ultar_i18n::datefmt;
use ultar_i18n::{Arg, Translator};

/// A moment in local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// The date.
    pub date: Date,
    /// Hour 0–23.
    pub hour: u8,
    /// Minute 0–59.
    pub minute: u8,
}

impl LocalTime {
    /// The local time of `unix_ms` at `offset_s`; `None` outside the calendar's range.
    pub fn at(unix_ms: u64, offset_s: i32) -> Option<LocalTime> {
        let secs = i64::try_from(unix_ms / 1000).ok()?;
        let date = Date::from_unix_seconds(secs, offset_s).ok()?;
        let local = secs.checked_add(i64::from(offset_s))?.rem_euclid(86_400);
        Some(LocalTime {
            date,
            hour: u8::try_from(local / 3600).ok()?,
            minute: u8::try_from((local % 3600) / 60).ok()?,
        })
    }

    /// `16:52`.
    pub fn clock(self) -> String {
        format!("{:02}:{:02}", self.hour % 24, self.minute % 60)
    }

    /// The date line of the home and lock screen: `الأربعاء، 25.11.` / `Wednesday, 25.11.`
    pub fn date_line(self, tr: &Translator) -> String {
        let weekday = datefmt::weekday_name(self.date.weekday(), tr);
        tr.t_args(
            "mobile.date",
            &[
                ("weekday", Arg::Str(&weekday)),
                ("day", Arg::Int(i64::from(self.date.day()))),
                ("month", Arg::Int(i64::from(self.date.month()))),
            ],
        )
    }

    /// `5 جمادى الآخرة 1448 هـ` (empty outside the Hijri calendar's range).
    pub fn hijri_line(self, tr: &Translator) -> String {
        let Ok(h) = self.date.to_hijri() else {
            return String::new();
        };
        let month = datefmt::hijri_month_name(h.month(), tr);
        tr.t_args(
            "mobile.hijri",
            &[
                ("day", Arg::Int(i64::from(h.day()))),
                ("month", Arg::Str(&month)),
                ("year", Arg::Int(i64::from(h.year()))),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;

    /// 25 November 2026, 19:56 UTC (a Wednesday).
    pub const SAMPLE_MS: u64 = 1_795_636_560_000;

    #[test]
    fn the_sample_moment_has_its_lines() {
        let t = LocalTime::at(SAMPLE_MS, 0).unwrap();
        assert_eq!(
            (t.date.month(), t.date.day(), t.clock()),
            (11, 25, "19:56".into())
        );
        let en = crate::i18n::translator(Lang::En).unwrap();
        assert_eq!(t.date_line(&en), "Wednesday, 25.11.");
        assert!(t.hijri_line(&en).contains("1448"));
        let ar = crate::i18n::translator(Lang::Ar).unwrap();
        assert!(t.date_line(&ar).starts_with("الأربعاء"));
    }

    #[test]
    fn an_offset_moves_the_day() {
        let t = LocalTime::at(SAMPLE_MS, 5 * 3600).unwrap();
        assert_eq!((t.date.day(), t.hour, t.minute), (26, 0, 56));
    }
}
