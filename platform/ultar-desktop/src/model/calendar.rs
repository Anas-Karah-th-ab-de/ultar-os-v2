//! The month grid of the calendar sheet: whole weeks from the first day of the week of the
//! language (Saturday in Arabic), days of the neighbour months muted.

use ultar_i18n::calendar::{days_in_month, Date, WeekStart};
use ultar_i18n::Translator;

/// One day of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Day {
    /// The date.
    pub date: Date,
    /// Belongs to the month shown.
    pub current: bool,
    /// Today.
    pub today: bool,
}

/// A month: year and month 1–12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Month {
    /// Year.
    pub year: i32,
    /// Month 1–12.
    pub month: u8,
}

impl Month {
    /// The month of a date.
    pub fn of(date: Date) -> Month {
        Month {
            year: date.year(),
            month: date.month(),
        }
    }

    /// The month before or after (`step` ±1).
    pub fn step(self, step: i32) -> Month {
        let index = self.year * 12 + i32::from(self.month) - 1 + step;
        Month {
            year: index.div_euclid(12),
            month: (index.rem_euclid(12) + 1) as u8,
        }
    }
}

/// The days of the grid of `month`: whole weeks, 35 or 42 cells.
pub fn grid(month: Month, today: Date, start: WeekStart) -> Vec<Day> {
    let Ok(first) = Date::new(month.year, month.month, 1) else {
        return Vec::new();
    };
    let lead = i64::from(first.weekday().position_in_week(start));
    let length = i64::from(days_in_month(month.year, month.month));
    let cells = if lead + length > 35 { 42 } else { 35 };
    let base = first.days_since_unix_epoch() - lead;
    (0..cells)
        .filter_map(|i| Date::from_days_since_unix_epoch(base + i).ok())
        .map(|date| Day {
            date,
            current: date.month() == month.month,
            today: date == today,
        })
        .collect()
}

/// `تشرين الثاني 2026`.
pub fn title(month: Month, tr: &Translator) -> String {
    let name = tr
        .t(&format!("desk.month.long.{}", month.month))
        .into_owned();
    tr.t_args(
        "desk.cal.title",
        &[
            ("month", ultar_i18n::Arg::Str(&name)),
            ("year", ultar_i18n::Arg::Int(i64::from(month.year))),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn november_2026_starts_on_sunday_after_one_saturday() {
        let today = Date::new(2026, 11, 22).unwrap();
        let g = grid(Month::of(today), today, WeekStart::Saturday);
        assert_eq!(g.len(), 35);
        // The board: 31 October first, then 1–30 November, then 1–4 December.
        assert_eq!(
            (g[0].date.month(), g[0].date.day(), g[0].current),
            (10, 31, false)
        );
        assert_eq!((g[1].date.day(), g[1].current), (1, true));
        assert!(g[22].today && g[22].date.day() == 22);
        assert_eq!((g[34].date.month(), g[34].date.day()), (12, 4));
        let monday = grid(Month::of(today), today, WeekStart::Monday);
        assert_eq!(monday[0].date.day(), 26);
        let long = grid(
            Month {
                year: 2026,
                month: 5,
            },
            today,
            WeekStart::Saturday,
        );
        assert_eq!(long.len(), 42);
        assert!(grid(
            Month {
                year: 2026,
                month: 13
            },
            today,
            WeekStart::Saturday
        )
        .is_empty());
    }

    #[test]
    fn months_step_over_years() {
        let m = Month {
            year: 2026,
            month: 1,
        };
        assert_eq!(
            m.step(-1),
            Month {
                year: 2025,
                month: 12
            }
        );
        assert_eq!(
            m.step(12),
            Month {
                year: 2027,
                month: 1
            }
        );
        let tr = crate::i18n::translator(ultar_i18n::Lang::Ar).unwrap();
        assert_eq!(
            title(
                Month {
                    year: 2026,
                    month: 11
                },
                &tr
            ),
            "تشرين الثاني 2026"
        );
    }
}
