//! Names and formatting of dates and times through the catalog keys `cal.*`.
//!
//! All numbers are written with Western digits 0–9, also in Arabic.

use alloc::format;
use alloc::string::String;

use crate::calendar::{Date, HijriDate, Weekday};
use crate::pattern::Arg;
use crate::translator::Translator;

/// Which Arabic names to use for Gregorian months. Has no effect in English and German.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MonthNames {
    /// Levantine names (كانون الثاني، شباط، آذار …) as used in Syria. Default.
    #[default]
    Levantine,
    /// Names derived from the Latin ones (يناير، فبراير، مارس …) as used in Egypt and the Gulf.
    Alternative,
}

/// Clock style for [`format_time`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClockStyle {
    /// 24-hour clock (`14:05`). Default.
    #[default]
    H24,
    /// 12-hour clock with a period marker (`2:05 م`).
    H12,
}

/// Name of a Gregorian month (1..=12). Returns the catalog key itself for other numbers.
pub fn month_name(month: u8, names: MonthNames, translator: &Translator) -> String {
    let key = match names {
        MonthNames::Levantine => format!("cal.month.{month}"),
        MonthNames::Alternative => format!("cal.month.alt.{month}"),
    };
    translator.t(&key).into_owned()
}

/// Name of a Hijri month (1..=12). Returns the catalog key itself for other numbers.
pub fn hijri_month_name(month: u8, translator: &Translator) -> String {
    translator
        .t(&format!("cal.hijri.month.{month}"))
        .into_owned()
}

/// Full name of a weekday.
pub fn weekday_name(day: Weekday, translator: &Translator) -> String {
    translator.t(&format!("cal.day.{}", day.id())).into_owned()
}

/// Short name of a weekday, for calendar headers.
pub fn weekday_short_name(day: Weekday, translator: &Translator) -> String {
    translator
        .t(&format!("cal.day.short.{}", day.id()))
        .into_owned()
}

/// Numeric date with two-digit day and month, e.g. `22/11/2026` (German `22.11.2026`).
pub fn format_date_numeric(date: Date, translator: &Translator) -> String {
    let day = format!("{:02}", date.day());
    let month = format!("{:02}", date.month());
    translator.t_args(
        "cal.format.numeric",
        &[
            ("day", Arg::Str(&day)),
            ("month", Arg::Str(&month)),
            ("year", Arg::Int(i64::from(date.year()))),
        ],
    )
}

/// Long date with the month name, e.g. `22 تشرين الثاني 2026`.
pub fn format_date_long(date: Date, names: MonthNames, translator: &Translator) -> String {
    let month = month_name(date.month(), names, translator);
    translator.t_args(
        "cal.format.long",
        &[
            ("day", Arg::Int(i64::from(date.day()))),
            ("month", Arg::Str(&month)),
            ("year", Arg::Int(i64::from(date.year()))),
        ],
    )
}

/// Long date with weekday, e.g. `الأحد، 22 تشرين الثاني 2026`.
pub fn format_date_with_weekday(date: Date, names: MonthNames, translator: &Translator) -> String {
    let month = month_name(date.month(), names, translator);
    let weekday = weekday_name(date.weekday(), translator);
    translator.t_args(
        "cal.format.long_weekday",
        &[
            ("weekday", Arg::Str(&weekday)),
            ("day", Arg::Int(i64::from(date.day()))),
            ("month", Arg::Str(&month)),
            ("year", Arg::Int(i64::from(date.year()))),
        ],
    )
}

/// Long Hijri date, e.g. `11 جمادى الآخرة 1448 هـ`.
pub fn format_hijri_long(date: HijriDate, translator: &Translator) -> String {
    let month = hijri_month_name(date.month(), translator);
    translator.t_args(
        "cal.format.hijri",
        &[
            ("day", Arg::Int(i64::from(date.day()))),
            ("month", Arg::Str(&month)),
            ("year", Arg::Int(i64::from(date.year()))),
        ],
    )
}

/// Time of day. `hour` is taken modulo 24 and `minute` modulo 60.
pub fn format_time(hour: u8, minute: u8, style: ClockStyle, translator: &Translator) -> String {
    let (hour, minute) = (hour % 24, minute % 60);
    match style {
        ClockStyle::H24 => format!("{hour:02}:{minute:02}"),
        ClockStyle::H12 => {
            let shown = match hour % 12 {
                0 => 12,
                other => other,
            };
            let time = format!("{shown}:{minute:02}");
            let period = translator.t(if hour < 12 {
                "cal.time.am"
            } else {
                "cal.time.pm"
            });
            translator.t_args(
                "cal.format.time12",
                &[("time", Arg::Str(&time)), ("period", Arg::Str(&period))],
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    fn translators() -> [Translator; 3] {
        Lang::ALL.map(Translator::new)
    }

    #[test]
    fn month_and_day_names() {
        let [ar, en, de] = translators();
        assert_eq!(month_name(11, MonthNames::Levantine, &ar), "تشرين الثاني");
        assert_eq!(month_name(11, MonthNames::Alternative, &ar), "نوفمبر");
        assert_eq!(month_name(3, MonthNames::Levantine, &de), "März");
        assert_eq!(month_name(3, MonthNames::Alternative, &en), "March");
        assert_eq!(month_name(13, MonthNames::Levantine, &en), "cal.month.13");
        assert_eq!(hijri_month_name(9, &ar), "رمضان");
        assert_eq!(hijri_month_name(9, &en), "Ramadan");
        assert_eq!(hijri_month_name(0, &en), "cal.hijri.month.0");
        assert_eq!(weekday_name(Weekday::Friday, &ar), "الجمعة");
        assert_eq!(weekday_name(Weekday::Friday, &de), "Freitag");
        assert_eq!(weekday_short_name(Weekday::Saturday, &ar), "سبت");
        assert_eq!(weekday_short_name(Weekday::Monday, &de), "Mo");
        assert_eq!(MonthNames::default(), MonthNames::Levantine);
    }

    #[test]
    fn dates_in_three_languages() {
        let [ar, en, de] = translators();
        let date = Date::new(2026, 11, 22).unwrap();
        assert_eq!(format_date_numeric(date, &ar), "22/11/2026");
        assert_eq!(format_date_numeric(date, &de), "22.11.2026");
        assert_eq!(
            format_date_numeric(Date::new(2026, 3, 5).unwrap(), &en),
            "05/03/2026"
        );
        assert_eq!(
            format_date_long(date, MonthNames::Levantine, &ar),
            "22 تشرين الثاني 2026"
        );
        assert_eq!(
            format_date_long(date, MonthNames::Levantine, &en),
            "22 November 2026"
        );
        assert_eq!(
            format_date_long(date, MonthNames::Levantine, &de),
            "22. November 2026"
        );
        assert_eq!(
            format_date_with_weekday(date, MonthNames::Alternative, &ar),
            "الأحد، 22 نوفمبر 2026"
        );
        assert_eq!(
            format_date_with_weekday(date, MonthNames::Levantine, &de),
            "Sonntag, 22. November 2026"
        );
    }

    #[test]
    fn hijri_dates() {
        let [ar, en, de] = translators();
        let hijri = Date::new(2026, 11, 22).unwrap().to_hijri().unwrap();
        assert_eq!(format_hijri_long(hijri, &ar), "11 جمادى الآخرة 1448 هـ");
        assert_eq!(
            format_hijri_long(hijri, &en),
            "11 Jumada al-Akhirah 1448 AH"
        );
        assert_eq!(
            format_hijri_long(hijri, &de),
            "11. Dschumada l-achira 1448 n. H."
        );
    }

    #[test]
    fn times() {
        let [ar, en, _] = translators();
        assert_eq!(format_time(14, 5, ClockStyle::H24, &ar), "14:05");
        assert_eq!(format_time(0, 0, ClockStyle::H24, &ar), "00:00");
        assert_eq!(format_time(14, 5, ClockStyle::H12, &ar), "2:05 م");
        assert_eq!(format_time(0, 30, ClockStyle::H12, &ar), "12:30 ص");
        assert_eq!(format_time(12, 0, ClockStyle::H12, &en), "12:00 PM");
        assert_eq!(format_time(11, 59, ClockStyle::H12, &en), "11:59 AM");
        assert_eq!(format_time(25, 61, ClockStyle::H24, &en), "01:01");
        assert_eq!(ClockStyle::default(), ClockStyle::H24);
    }

    #[test]
    fn formatted_dates_use_western_digits_only() {
        let ar = Translator::new(Lang::Ar);
        let date = Date::new(2026, 11, 22).unwrap();
        let texts = [
            format_date_numeric(date, &ar),
            format_date_long(date, MonthNames::Levantine, &ar),
            format_hijri_long(date.to_hijri().unwrap(), &ar),
            format_time(9, 7, ClockStyle::H12, &ar),
        ];
        for text in texts {
            assert!(
                !text
                    .chars()
                    .any(|c| crate::numbers::western_digit(c).is_some()),
                "{text}"
            );
        }
    }
}
