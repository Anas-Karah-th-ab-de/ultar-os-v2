//! Calendar arithmetic: Gregorian dates, weekdays, week start and the tabular (civil) Hijri calendar.
//!
//! The Hijri conversion uses the arithmetic civil calendar (epoch Friday 16 July 622 Julian, leap
//! years 2, 5, 7, 10, 13, 16, 18, 21, 24, 26 and 29 of each 30-year cycle). Calendars based on moon
//! sighting, and Umm al-Qura, can differ from it by one or two days.
//!
//! Names of months and days live in the catalogs; see [`crate::datefmt`].

use core::fmt;

use crate::lang::Lang;

/// Julian day number of 1970-01-01.
const JDN_UNIX_EPOCH: i64 = 2_440_588;
/// Julian day number of 1 Muharram 1 AH (civil epoch).
const JDN_HIJRI_EPOCH: i64 = 1_948_440;
/// Days in a 30-year Hijri cycle.
const HIJRI_CYCLE_DAYS: i64 = 10_631;

/// Error of the calendar functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateError {
    /// The month is not in 1..=12.
    InvalidMonth,
    /// The day does not exist in that month.
    InvalidDay,
    /// The date lies before the start of the Hijri calendar.
    BeforeHijriEpoch,
    /// The year is outside the supported range (±1,000,000).
    YearOutOfRange,
}

impl DateError {
    /// Message key that describes this error in the catalogs.
    pub const fn message_key(self) -> &'static str {
        match self {
            DateError::InvalidMonth => "i18n.date.invalid_month",
            DateError::InvalidDay => "i18n.date.invalid_day",
            DateError::BeforeHijriEpoch => "i18n.date.before_hijri_epoch",
            DateError::YearOutOfRange => "i18n.date.year_out_of_range",
        }
    }
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DateError::InvalidMonth => "invalid month",
            DateError::InvalidDay => "invalid day",
            DateError::BeforeHijriEpoch => "date before the Hijri epoch",
            DateError::YearOutOfRange => "year out of range",
        })
    }
}

impl core::error::Error for DateError {}

/// A day of the week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weekday {
    /// Sunday.
    Sunday,
    /// Monday.
    Monday,
    /// Tuesday.
    Tuesday,
    /// Wednesday.
    Wednesday,
    /// Thursday.
    Thursday,
    /// Friday.
    Friday,
    /// Saturday.
    Saturday,
}

impl Weekday {
    /// All weekdays starting with Sunday.
    pub const ALL: [Weekday; 7] = [
        Weekday::Sunday,
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
    ];

    /// Number of the day counted from Sunday = 0.
    pub const fn number_from_sunday(self) -> u8 {
        self as u8
    }

    /// The weekday with the given number counted from Sunday = 0 (taken modulo 7).
    pub const fn from_number(number: u8) -> Weekday {
        Weekday::ALL[(number % 7) as usize]
    }

    /// Short English identifier used in catalog keys (`sun`, `mon`, …).
    pub const fn id(self) -> &'static str {
        match self {
            Weekday::Sunday => "sun",
            Weekday::Monday => "mon",
            Weekday::Tuesday => "tue",
            Weekday::Wednesday => "wed",
            Weekday::Thursday => "thu",
            Weekday::Friday => "fri",
            Weekday::Saturday => "sat",
        }
    }

    /// Position of this day in a week that starts on `start` (0..=6).
    pub const fn position_in_week(self, start: WeekStart) -> u8 {
        (self.number_from_sunday() + 7 - start.first_day().number_from_sunday()) % 7
    }
}

/// The first day of the week. Configurable by the user; the default depends on the language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeekStart {
    /// Week starts on Saturday (default for Arabic).
    Saturday,
    /// Week starts on Sunday (default for English).
    Sunday,
    /// Week starts on Monday (default for German).
    Monday,
}

impl WeekStart {
    /// Default week start of a language.
    pub const fn default_for(lang: Lang) -> WeekStart {
        match lang {
            Lang::Ar => WeekStart::Saturday,
            Lang::En => WeekStart::Sunday,
            Lang::De => WeekStart::Monday,
        }
    }

    /// The weekday on which the week starts.
    pub const fn first_day(self) -> Weekday {
        match self {
            WeekStart::Saturday => Weekday::Saturday,
            WeekStart::Sunday => Weekday::Sunday,
            WeekStart::Monday => Weekday::Monday,
        }
    }

    /// The seven weekdays in the order of a calendar row.
    pub const fn days(self) -> [Weekday; 7] {
        let first = self.first_day().number_from_sunday();
        let mut days = [Weekday::Sunday; 7];
        let mut i = 0;
        while i < 7 {
            days[i as usize] = Weekday::from_number(first + i);
            i += 1;
        }
        days
    }

    /// Parses a setting value: `saturday`/`sat`, `sunday`/`sun`, `monday`/`mon` (case-insensitive).
    pub fn parse(text: &str) -> Option<WeekStart> {
        let text = text.trim();
        let is = |long: &str, short: &str| {
            text.eq_ignore_ascii_case(long) || text.eq_ignore_ascii_case(short)
        };
        if is("saturday", "sat") {
            Some(WeekStart::Saturday)
        } else if is("sunday", "sun") {
            Some(WeekStart::Sunday)
        } else if is("monday", "mon") {
            Some(WeekStart::Monday)
        } else {
            None
        }
    }
}

/// A date of the (proleptic) Gregorian calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

const MAX_YEAR: i32 = 1_000_000;

/// `true` if `year` is a Gregorian leap year.
pub const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days of a Gregorian month; 0 if the month is not in 1..=12.
pub const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    /// Creates a date after checking that it exists.
    pub fn new(year: i32, month: u8, day: u8) -> Result<Date, DateError> {
        if !(-MAX_YEAR..=MAX_YEAR).contains(&year) {
            return Err(DateError::YearOutOfRange);
        }
        if !(1..=12).contains(&month) {
            return Err(DateError::InvalidMonth);
        }
        if day == 0 || day > days_in_month(year, month) {
            return Err(DateError::InvalidDay);
        }
        Ok(Date { year, month, day })
    }

    /// The year.
    pub const fn year(self) -> i32 {
        self.year
    }

    /// The month, 1..=12.
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month, 1..=31.
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Days since 1970-01-01 (negative before).
    pub const fn days_since_unix_epoch(self) -> i64 {
        let year = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = year.div_euclid(400);
        let year_of_era = year.rem_euclid(400);
        let shifted_month = (self.month as i64 + 9) % 12;
        let day_of_year = (153 * shifted_month + 2) / 5 + self.day as i64 - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The date that lies `days` after 1970-01-01.
    pub fn from_days_since_unix_epoch(days: i64) -> Result<Date, DateError> {
        let shifted = days.checked_add(719_468).ok_or(DateError::YearOutOfRange)?;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        let year = i32::try_from(year).map_err(|_| DateError::YearOutOfRange)?;
        // month is 1..=12 and day 1..=31 by construction.
        Date::new(year, month as u8, day as u8)
    }

    /// The date of a Unix timestamp (seconds since 1970-01-01 UTC) shifted by `offset_seconds`
    /// (for example `3 * 3600` for Damascus).
    pub fn from_unix_seconds(seconds: i64, offset_seconds: i32) -> Result<Date, DateError> {
        let local = seconds.saturating_add(i64::from(offset_seconds));
        Date::from_days_since_unix_epoch(local.div_euclid(86_400))
    }

    /// Julian day number of the date.
    pub const fn julian_day_number(self) -> i64 {
        self.days_since_unix_epoch() + JDN_UNIX_EPOCH
    }

    /// The day of the week.
    pub const fn weekday(self) -> Weekday {
        // 1970-01-01 was a Thursday (number 4 from Sunday).
        Weekday::from_number((self.days_since_unix_epoch() + 4).rem_euclid(7) as u8)
    }

    /// The same day in the tabular Hijri calendar.
    pub fn to_hijri(self) -> Result<HijriDate, DateError> {
        HijriDate::from_julian_day_number(self.julian_day_number())
    }
}

/// A date of the tabular (civil) Hijri calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HijriDate {
    year: i32,
    month: u8,
    day: u8,
}

/// `true` if the Hijri year has 355 days.
pub const fn is_hijri_leap_year(year: i32) -> bool {
    (11 * year as i64 + 14).rem_euclid(30) < 11
}

/// Number of days of a Hijri month; 0 if the month is not in 1..=12.
pub const fn days_in_hijri_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 9 | 11 => 30,
        2 | 4 | 6 | 8 | 10 => 29,
        12 if is_hijri_leap_year(year) => 30,
        12 => 29,
        _ => 0,
    }
}

/// Days from the Hijri epoch to the first day of `year`.
const fn hijri_days_before_year(year: i64) -> i64 {
    354 * (year - 1) + (11 * year + 3).div_euclid(30)
}

impl HijriDate {
    /// Creates a Hijri date after checking that it exists (years from 1 AH).
    pub fn new(year: i32, month: u8, day: u8) -> Result<HijriDate, DateError> {
        if year < 1 {
            return Err(DateError::BeforeHijriEpoch);
        }
        if year > MAX_YEAR {
            return Err(DateError::YearOutOfRange);
        }
        if !(1..=12).contains(&month) {
            return Err(DateError::InvalidMonth);
        }
        if day == 0 || day > days_in_hijri_month(year, month) {
            return Err(DateError::InvalidDay);
        }
        Ok(HijriDate { year, month, day })
    }

    /// The Hijri year.
    pub const fn year(self) -> i32 {
        self.year
    }

    /// The month, 1 (Muharram) ..= 12 (Dhu al-Hijjah).
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month, 1..=30.
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Julian day number of the date.
    pub const fn julian_day_number(self) -> i64 {
        let months = self.month as i64 - 1;
        let days_before_month = 29 * months + (months + 1) / 2;
        JDN_HIJRI_EPOCH
            + hijri_days_before_year(self.year as i64)
            + days_before_month
            + self.day as i64
            - 1
    }

    /// The Hijri date of a Julian day number.
    pub fn from_julian_day_number(jdn: i64) -> Result<HijriDate, DateError> {
        let days = jdn - JDN_HIJRI_EPOCH;
        if days < 0 {
            return Err(DateError::BeforeHijriEpoch);
        }
        let mut year = (30 * days + 10_646) / HIJRI_CYCLE_DAYS;
        while hijri_days_before_year(year) > days {
            year -= 1;
        }
        while hijri_days_before_year(year + 1) <= days {
            year += 1;
        }
        let year_i32 = i32::try_from(year).map_err(|_| DateError::YearOutOfRange)?;
        let mut remaining = days - hijri_days_before_year(year);
        let mut month = 1u8;
        while month < 12 && remaining >= i64::from(days_in_hijri_month(year_i32, month)) {
            remaining -= i64::from(days_in_hijri_month(year_i32, month));
            month += 1;
        }
        // remaining is 0..=29 here.
        HijriDate::new(year_i32, month, remaining as u8 + 1)
    }

    /// The same day in the Gregorian calendar.
    pub fn to_gregorian(self) -> Result<Date, DateError> {
        Date::from_days_since_unix_epoch(self.julian_day_number() - JDN_UNIX_EPOCH)
    }
}

#[cfg(test)]
#[path = "calendar_tests.rs"]
mod tests;
