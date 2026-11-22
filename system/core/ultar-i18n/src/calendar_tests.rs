//! Tests of the calendar arithmetic.

use super::*;

fn date(y: i32, m: u8, d: u8) -> Date {
    Date::new(y, m, d).unwrap()
}

fn hijri(y: i32, m: u8, d: u8) -> HijriDate {
    HijriDate::new(y, m, d).unwrap()
}

#[test]
fn gregorian_validation() {
    assert!(Date::new(2024, 2, 29).is_ok());
    assert_eq!(Date::new(2023, 2, 29), Err(DateError::InvalidDay));
    assert_eq!(Date::new(1900, 2, 29), Err(DateError::InvalidDay));
    assert!(Date::new(2000, 2, 29).is_ok());
    assert_eq!(Date::new(2024, 0, 1), Err(DateError::InvalidMonth));
    assert_eq!(Date::new(2024, 13, 1), Err(DateError::InvalidMonth));
    assert_eq!(Date::new(2024, 4, 31), Err(DateError::InvalidDay));
    assert_eq!(Date::new(2024, 4, 0), Err(DateError::InvalidDay));
    assert_eq!(Date::new(2_000_000, 1, 1), Err(DateError::YearOutOfRange));
    assert_eq!(days_in_month(2024, 13), 0);
    let d = date(2026, 11, 22);
    assert_eq!((d.year(), d.month(), d.day()), (2026, 11, 22));
}

#[test]
fn days_and_julian_day_numbers() {
    assert_eq!(date(1970, 1, 1).days_since_unix_epoch(), 0);
    assert_eq!(date(1969, 12, 31).days_since_unix_epoch(), -1);
    assert_eq!(date(2000, 1, 1).days_since_unix_epoch(), 10_957);
    assert_eq!(date(2000, 1, 1).julian_day_number(), 2_451_545);
    assert_eq!(date(1858, 11, 17).julian_day_number(), 2_400_001);
    assert_eq!(Date::from_days_since_unix_epoch(0), Ok(date(1970, 1, 1)));
    assert_eq!(
        Date::from_days_since_unix_epoch(19_782),
        Ok(date(2024, 2, 29))
    );
    assert_eq!(
        Date::from_days_since_unix_epoch(i64::MAX),
        Err(DateError::YearOutOfRange)
    );
    assert_eq!(
        Date::from_days_since_unix_epoch(i64::MAX / 2),
        Err(DateError::YearOutOfRange)
    );
}

#[test]
fn gregorian_round_trip_over_eight_centuries() {
    let start = date(1600, 1, 1).days_since_unix_epoch();
    let end = date(2400, 12, 31).days_since_unix_epoch();
    let mut previous = Date::from_days_since_unix_epoch(start - 1).unwrap();
    for days in start..=end {
        let current = Date::from_days_since_unix_epoch(days).unwrap();
        assert_eq!(current.days_since_unix_epoch(), days);
        assert!(current > previous);
        previous = current;
    }
}

#[test]
fn unix_seconds_with_offset() {
    assert_eq!(Date::from_unix_seconds(0, 0), Ok(date(1970, 1, 1)));
    assert_eq!(Date::from_unix_seconds(-1, 0), Ok(date(1969, 12, 31)));
    // 2026-11-22 22:30 UTC is already the 23rd in Damascus (UTC+3).
    let evening = date(2026, 11, 22).days_since_unix_epoch() * 86_400 + 22 * 3600 + 1800;
    assert_eq!(Date::from_unix_seconds(evening, 0), Ok(date(2026, 11, 22)));
    assert_eq!(
        Date::from_unix_seconds(evening, 3 * 3600),
        Ok(date(2026, 11, 23))
    );
}

#[test]
fn weekdays_of_known_dates() {
    assert_eq!(date(1970, 1, 1).weekday(), Weekday::Thursday);
    assert_eq!(date(2000, 1, 1).weekday(), Weekday::Saturday);
    assert_eq!(date(2024, 2, 29).weekday(), Weekday::Thursday);
    assert_eq!(date(2026, 11, 22).weekday(), Weekday::Sunday);
    assert_eq!(date(1969, 7, 20).weekday(), Weekday::Sunday);
    assert_eq!(date(1900, 1, 1).weekday(), Weekday::Monday);
}

#[test]
fn weekday_numbers_and_ids() {
    for (number, day) in Weekday::ALL.iter().enumerate() {
        assert_eq!(day.number_from_sunday() as usize, number);
        assert_eq!(Weekday::from_number(number as u8), *day);
        assert_eq!(day.id().len(), 3);
    }
    assert_eq!(Weekday::from_number(7), Weekday::Sunday);
    assert_eq!(Weekday::from_number(255), Weekday::Wednesday);
}

#[test]
fn week_start_defaults_and_order() {
    assert_eq!(WeekStart::default_for(Lang::Ar), WeekStart::Saturday);
    assert_eq!(WeekStart::default_for(Lang::En), WeekStart::Sunday);
    assert_eq!(WeekStart::default_for(Lang::De), WeekStart::Monday);
    let arabic_week = WeekStart::Saturday.days();
    assert_eq!(arabic_week[0], Weekday::Saturday);
    assert_eq!(arabic_week[1], Weekday::Sunday);
    assert_eq!(arabic_week[6], Weekday::Friday);
    assert_eq!(WeekStart::Monday.days()[6], Weekday::Sunday);
    assert_eq!(WeekStart::Sunday.days(), Weekday::ALL);
    for start in [WeekStart::Saturday, WeekStart::Sunday, WeekStart::Monday] {
        for (position, day) in start.days().iter().enumerate() {
            assert_eq!(day.position_in_week(start) as usize, position);
        }
    }
}

#[test]
fn week_start_parsing() {
    assert_eq!(WeekStart::parse("saturday"), Some(WeekStart::Saturday));
    assert_eq!(WeekStart::parse(" SUN "), Some(WeekStart::Sunday));
    assert_eq!(WeekStart::parse("Monday"), Some(WeekStart::Monday));
    assert_eq!(WeekStart::parse("friday"), None);
    assert_eq!(WeekStart::parse(""), None);
}

#[test]
fn hijri_known_dates() {
    // Reference values of the tabular civil calendar (epoch 16 July 622 Julian = 19 July 622 Gregorian).
    let known = [
        ((622, 7, 19), (1, 1, 1)),
        ((1970, 1, 1), (1389, 10, 22)),
        ((2000, 1, 1), (1420, 9, 24)),
        ((2023, 7, 19), (1445, 1, 1)),
        ((2024, 7, 7), (1445, 12, 30)),
        ((2024, 7, 8), (1446, 1, 1)),
        ((1990, 8, 2), (1411, 1, 10)),
        ((2024, 3, 11), (1445, 9, 1)),
        ((2025, 3, 1), (1446, 9, 1)),
        ((2026, 11, 22), (1448, 6, 11)),
    ];
    for ((gy, gm, gd), (hy, hm, hd)) in known {
        let gregorian = date(gy, gm, gd);
        assert_eq!(
            gregorian.to_hijri(),
            Ok(hijri(hy, hm, hd)),
            "{gy}-{gm}-{gd}"
        );
        assert_eq!(hijri(hy, hm, hd).to_gregorian(), Ok(gregorian));
    }
    assert_eq!(hijri(1, 1, 1).julian_day_number(), 1_948_440);
    assert_eq!(date(622, 7, 19).weekday(), Weekday::Friday);
}

#[test]
fn hijri_validation_and_leap_years() {
    let leap: [i32; 11] = [2, 5, 7, 10, 13, 16, 18, 21, 24, 26, 29];
    for year in 1..=30 {
        assert_eq!(
            is_hijri_leap_year(year),
            leap.contains(&year),
            "year {year}"
        );
        assert_eq!(is_hijri_leap_year(year + 30), leap.contains(&year));
    }
    assert!(HijriDate::new(1445, 12, 30).is_ok());
    assert_eq!(HijriDate::new(1446, 12, 30), Err(DateError::InvalidDay));
    assert_eq!(HijriDate::new(1446, 2, 30), Err(DateError::InvalidDay));
    assert_eq!(HijriDate::new(1446, 1, 0), Err(DateError::InvalidDay));
    assert_eq!(HijriDate::new(1446, 13, 1), Err(DateError::InvalidMonth));
    assert_eq!(HijriDate::new(0, 1, 1), Err(DateError::BeforeHijriEpoch));
    assert_eq!(
        HijriDate::new(2_000_000, 1, 1),
        Err(DateError::YearOutOfRange)
    );
    assert_eq!(
        date(622, 7, 18).to_hijri(),
        Err(DateError::BeforeHijriEpoch)
    );
    assert_eq!(days_in_hijri_month(1446, 0), 0);
    let h = hijri(1448, 6, 12);
    assert_eq!((h.year(), h.month(), h.day()), (1448, 6, 12));
}

#[test]
fn hijri_round_trip_and_continuity() {
    // Every day from 1 AH to beyond 1700 AH: conversion is the inverse and advances by one day.
    let first = hijri(1, 1, 1).julian_day_number();
    let mut previous: Option<HijriDate> = None;
    for jdn in first..first + 603_000 {
        let current = HijriDate::from_julian_day_number(jdn).unwrap();
        assert_eq!(current.julian_day_number(), jdn);
        if let Some(before) = previous {
            let same_month = (current.year(), current.month()) == (before.year(), before.month());
            if same_month {
                assert_eq!(current.day(), before.day() + 1);
            } else {
                assert_eq!(current.day(), 1);
                assert_eq!(
                    before.day(),
                    days_in_hijri_month(before.year(), before.month())
                );
            }
        }
        previous = Some(current);
    }
    assert!(previous.unwrap().year() > 1700);
}

#[test]
fn hijri_year_lengths() {
    for year in 1..=3000 {
        let length =
            hijri(year + 1, 1, 1).julian_day_number() - hijri(year, 1, 1).julian_day_number();
        assert_eq!(length, if is_hijri_leap_year(year) { 355 } else { 354 });
    }
}

#[test]
fn errors_have_keys_and_text() {
    let all = [
        DateError::InvalidMonth,
        DateError::InvalidDay,
        DateError::BeforeHijriEpoch,
        DateError::YearOutOfRange,
    ];
    for error in all {
        assert!(error.message_key().starts_with("i18n.date."));
        assert!(!alloc::format!("{error}").is_empty());
    }
}
