//! The time shown by the bar and the lock screen: an injectable clock, the bar's two lines,
//! the lock screen's date with its Hijri date. Digits are always 0–9 (DESKTOP-SPEC §3).

use ultar_i18n::calendar::{Date, Weekday};
use ultar_i18n::datefmt::{self, ClockStyle};
use ultar_i18n::{Arg, Translator};

/// Wall-clock time: milliseconds since the epoch and the zone's offset.
pub trait Clock: Send + Sync {
    /// Now, in milliseconds since the epoch.
    fn now_ms(&self) -> u64;
    /// Offset of the local time zone in seconds.
    fn offset_s(&self) -> i32 {
        0
    }
}

/// The system clock (UTC until the time zone setting is wired: `datetime.timezone.name`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock {
    /// Offset of the local zone in seconds.
    pub offset_s: i32,
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    fn offset_s(&self) -> i32 {
        self.offset_s
    }
}

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
            hour: (local / 3600) as u8,
            minute: ((local % 3600) / 60) as u8,
        })
    }

    /// Milliseconds until the minute changes (when the bar repaints its clock).
    pub fn ms_to_next_minute(unix_ms: u64) -> u64 {
        60_000 - unix_ms % 60_000
    }
}

fn month_key(kind: &str, month: u8) -> String {
    format!("desk.month.{kind}.{month}")
}

fn weekday_key(kind: &str, day: Weekday) -> String {
    format!("desk.weekday.{kind}.{}", day.id())
}

/// The bar's two lines: the time and the short date (boards: `16:52` / `الأحد 22 تشرين
/// الثاني`, `Sun 22 Nov`).
pub fn bar_lines(t: LocalTime, style: ClockStyle, tr: &Translator) -> (String, String) {
    let time = datefmt::format_time(t.hour, t.minute, style, tr);
    let weekday = tr.t(&weekday_key("bar", t.date.weekday())).into_owned();
    let month = tr.t(&month_key("bar", t.date.month())).into_owned();
    let day = i64::from(t.date.day());
    let date = tr.t_args(
        "desk.bar.date",
        &[
            ("weekday", Arg::Str(&weekday)),
            ("day", Arg::Int(day)),
            ("month", Arg::Str(&month)),
        ],
    );
    (time, date)
}

/// The lock screen's date line and Hijri line (board Lock: `الأحد 22 تشرين الثاني 2026`,
/// `21 جمادى الآخرة 1448`).
pub fn lock_lines(t: LocalTime, tr: &Translator) -> (String, String) {
    let weekday = datefmt::weekday_name(t.date.weekday(), tr);
    let month = tr.t(&month_key("long", t.date.month())).into_owned();
    let date = tr.t_args(
        "lock.date",
        &[
            ("weekday", Arg::Str(&weekday)),
            ("day", Arg::Int(i64::from(t.date.day()))),
            ("month", Arg::Str(&month)),
            ("year", Arg::Int(i64::from(t.date.year()))),
        ],
    );
    (date, hijri_line(t.date, tr))
}

/// `21 جمادى الآخرة 1448` (empty outside the Hijri calendar's range).
pub fn hijri_line(date: Date, tr: &Translator) -> String {
    let Ok(h) = date.to_hijri() else {
        return String::new();
    };
    let month = datefmt::hijri_month_name(h.month(), tr);
    tr.t_args(
        "desk.hijri",
        &[
            ("day", Arg::Int(i64::from(h.day()))),
            ("month", Arg::Str(&month)),
            ("year", Arg::Int(i64::from(h.year()))),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;

    /// 22 November 2026, 16:52 UTC (a Sunday).
    pub const BOARD_TIME_MS: u64 = 1_795_366_320_000;

    #[test]
    fn the_board_moment_formats_like_the_boards() {
        let t = LocalTime::at(BOARD_TIME_MS, 0).unwrap();
        assert_eq!(
            (t.date.year(), t.date.month(), t.date.day()),
            (2026, 11, 22)
        );
        assert_eq!((t.hour, t.minute), (16, 52));
        let ar = crate::i18n::translator(Lang::Ar).unwrap();
        let (time, date) = bar_lines(t, ClockStyle::H24, &ar);
        assert_eq!(time, "16:52");
        assert_eq!(date, "الأحد 22 تشرين الثاني");
        let en = crate::i18n::translator(Lang::En).unwrap();
        assert_eq!(bar_lines(t, ClockStyle::H24, &en).1, "Sun 22 Nov");
        let (long, hijri) = lock_lines(t, &ar);
        assert_eq!(long, "الأحد 22 تشرين الثاني 2026");
        assert!(
            hijri.contains("جمادى الآخرة") && hijri.contains("1448"),
            "{hijri}"
        );
        assert_eq!(LocalTime::ms_to_next_minute(BOARD_TIME_MS), 60_000);
        assert_eq!(LocalTime::at(BOARD_TIME_MS, 3 * 3600).unwrap().hour, 19);
        assert!(SystemClock::default().now_ms() > 0);
    }
}
