//! One person's events, without the bus.

use serde::{Deserialize, Serialize};
use ultar_bus::messages::calendar::{
    Add, CalendarOutcome, Date, DayEvent, DayReply, MAX_DAY_EVENTS, MAX_EVENTS, MAX_TITLE,
};

/// Minutes of a day.
pub const DAY_MINUTES: u16 = 1440;

/// Days of `month` in `year` (Gregorian).
pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// A real day between 1970 and 2200.
pub fn valid_date(date: Date) -> bool {
    (1970..=2200).contains(&date.year)
        && (1..=12).contains(&date.month)
        && date.day >= 1
        && date.day <= days_in_month(date.year, date.month)
}

/// One stored event.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Event {
    /// Id.
    pub id: u64,
    /// Day.
    pub date: Date,
    /// Start minute.
    pub start_minute: u16,
    /// Length in minutes.
    pub duration_minutes: u16,
    /// Title.
    pub title: String,
}

/// One person's events.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Book {
    next_id: u64,
    events: Vec<Event>,
}

impl Book {
    /// A book read from disk is checked before it is used.
    pub fn is_valid(&self) -> bool {
        self.events.len() <= MAX_EVENTS
            && self
                .events
                .iter()
                .all(|e| e.id <= self.next_id && valid_date(e.date) && e.start_minute < DAY_MINUTES)
    }

    /// The events of `date` by start time.
    pub fn day(&self, date: Date) -> DayReply {
        let mut all: Vec<&Event> = self.events.iter().filter(|e| e.date == date).collect();
        all.sort_by_key(|e| (e.start_minute, e.id));
        DayReply {
            more: all.len() > MAX_DAY_EVENTS,
            events: all
                .into_iter()
                .take(MAX_DAY_EVENTS)
                .map(|e| DayEvent {
                    id: e.id,
                    start_minute: e.start_minute,
                    duration_minutes: e.duration_minutes,
                    title: e.title.clone(),
                })
                .collect(),
        }
    }

    /// Adds an event (checked).
    pub fn add(&mut self, add: Add) -> CalendarOutcome {
        let title_ok = !add.title.trim().is_empty()
            && add.title.len() <= MAX_TITLE
            && !add.title.chars().any(char::is_control);
        if !valid_date(add.date)
            || add.start_minute >= DAY_MINUTES
            || add.duration_minutes > DAY_MINUTES
            || !title_ok
        {
            return CalendarOutcome::Invalid;
        }
        if self.events.len() >= MAX_EVENTS {
            return CalendarOutcome::Unavailable;
        }
        self.next_id += 1;
        self.events.push(Event {
            id: self.next_id,
            date: add.date,
            start_minute: add.start_minute,
            duration_minutes: add.duration_minutes,
            title: add.title,
        });
        CalendarOutcome::Done(self.next_id)
    }

    /// Removes an event.
    pub fn remove(&mut self, id: u64) -> CalendarOutcome {
        match self.events.iter().position(|e| e.id == id) {
            Some(i) => {
                self.events.remove(i);
                CalendarOutcome::Done(id)
            }
            None => CalendarOutcome::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: u16, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    fn add(d: Date, start: u16, title: &str) -> Add {
        Add {
            date: d,
            start_minute: start,
            duration_minutes: 30,
            title: title.into(),
        }
    }

    #[test]
    fn calendar_rules() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 13), 0);
        assert!(valid_date(date(2026, 11, 30)) && !valid_date(date(2026, 11, 31)));
        assert!(!valid_date(date(1969, 1, 1)) && !valid_date(date(2026, 0, 1)));
    }

    #[test]
    fn add_list_remove() {
        let mut b = Book::default();
        let d = date(2026, 11, 24);
        assert_eq!(b.add(add(d, 600, "b")), CalendarOutcome::Done(1));
        assert_eq!(b.add(add(d, 540, "a")), CalendarOutcome::Done(2));
        let day = b.day(d);
        assert_eq!(
            day.events
                .iter()
                .map(|e| e.title.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        for bad in [
            add(d, 1440, "x"),
            add(d, 0, " "),
            add(d, 0, "a\nb"),
            add(date(2026, 2, 30), 0, "x"),
            add(d, 0, &"x".repeat(MAX_TITLE + 1)),
        ] {
            assert_eq!(b.add(bad), CalendarOutcome::Invalid);
        }
        let mut long = add(d, 0, "x");
        long.duration_minutes = DAY_MINUTES + 1;
        assert_eq!(b.add(long), CalendarOutcome::Invalid);
        assert_eq!(b.remove(1), CalendarOutcome::Done(1));
        assert_eq!(b.remove(1), CalendarOutcome::Unknown);
        assert!(b.is_valid());
    }

    #[test]
    fn limits() {
        let mut b = Book::default();
        let d = date(2026, 1, 1);
        for i in 0..MAX_EVENTS {
            assert!(matches!(
                b.add(add(d, (i % 1440) as u16, "e")),
                CalendarOutcome::Done(_)
            ));
        }
        assert_eq!(b.add(add(d, 0, "e")), CalendarOutcome::Unavailable);
        let day = b.day(d);
        assert!(day.more && day.events.len() == MAX_DAY_EVENTS);
        let bad = Book {
            next_id: 0,
            events: vec![Event {
                id: 5,
                date: d,
                start_minute: 0,
                duration_minutes: 0,
                title: "x".into(),
            }],
        };
        assert!(!bad.is_valid());
    }
}
