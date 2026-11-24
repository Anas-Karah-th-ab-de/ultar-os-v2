//! When a call of the history happened: "Today 16:20", "Yesterday 19:42", "20 November".
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use ultar_i18n::datefmt::{format_date_long, format_time, ClockStyle, MonthNames};
use ultar_i18n::{Date, Translator};
use ultar_ui::widgets::Label;

const DAY: i64 = 86_400;

/// Label for a call at `when_s` seen at `now_s` in a zone `offset_s` east of UTC.
pub fn when_label(when_s: i64, now_s: i64, offset_s: i32, tr: &Translator) -> Label {
    let local = when_s.saturating_add(i64::from(offset_s));
    let (hour, minute) = (
        u8::try_from(local.rem_euclid(DAY) / 3600).unwrap_or(0),
        u8::try_from(local.rem_euclid(3600) / 60).unwrap_or(0),
    );
    let time = format_time(hour, minute, ClockStyle::H24, tr);
    let day = |s: i64| Date::from_unix_seconds(s, offset_s).map(Date::days_since_unix_epoch);
    match (day(when_s), day(now_s)) {
        (Ok(d), Ok(n)) if d == n => Label::key_with("phone.when.today", &[("time", &time)]),
        (Ok(d), Ok(n)) if d + 1 == n => Label::key_with("phone.when.yesterday", &[("time", &time)]),
        _ => match Date::from_unix_seconds(when_s, offset_s) {
            Ok(date) => Label::data(&format_date_long(date, MonthNames::Levantine, tr)),
            Err(_) => Label::data(&time),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;

    fn text(l: &Label, tr: &Translator) -> String {
        l.resolve(tr).into_owned()
    }

    #[test]
    fn today_yesterday_and_older() {
        let tr = crate::text::translator(Lang::En).unwrap();
        let now = 1_763_820_000; // 2025-11-22 14:00 UTC
        let t = text(&when_label(now - 3600, now, 0, &tr), &tr);
        assert!(t.starts_with("Today") && t.contains("13:00"), "{t}");
        let t = text(&when_label(now - DAY, now, 0, &tr), &tr);
        assert!(t.starts_with("Yesterday"), "{t}");
        let t = text(&when_label(now - 3 * DAY, now, 0, &tr), &tr);
        assert!(t.contains("19") && t.contains("2025"), "{t}");
        // Time zone moves the hour.
        let t = text(&when_label(now, now, 3 * 3600, &tr), &tr);
        assert!(t.contains("17:00"), "{t}");
        let ar = crate::text::translator(Lang::Ar).unwrap();
        let t = text(&when_label(now, now, 0, &ar), &ar);
        assert!(t.contains("اليوم"), "{t}");
    }
}
