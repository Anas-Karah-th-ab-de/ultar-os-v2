//! Contract of the calendar service: the events of the person in front.
//!
//! The store belongs to the signed-in person who is in front (`session.state`); with
//! nobody in front, days are empty and nothing can be added. Titles are data written by
//! a person, not interface text.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "calendar";
/// Most events one day answer lists.
pub const MAX_DAY_EVENTS: usize = 16;
/// Longest title in bytes.
pub const MAX_TITLE: usize = 256;
/// Most events one person keeps.
pub const MAX_EVENTS: usize = 4096;

/// A day of the Gregorian calendar.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct Date {
    /// Year (1970–2200).
    pub year: u16,
    /// Month (1–12).
    pub month: u8,
    /// Day of the month (1–31, checked against the month).
    pub day: u8,
}

/// One event as a day lists it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct DayEvent {
    /// Id of the event.
    pub id: u64,
    /// Start in minutes after midnight (0–1439).
    pub start_minute: u16,
    /// Length in minutes (0: no length).
    pub duration_minutes: u16,
    /// Title.
    pub title: String,
}

/// `calendar.day` – the events of one day, by start time (at most [`MAX_DAY_EVENTS`]).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Day {
    /// The day.
    pub date: Date,
}

/// Answer to [`Day`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct DayReply {
    /// The events.
    pub events: Vec<DayEvent>,
    /// More events exist than were listed.
    pub more: bool,
}

/// `calendar.add` – a new event.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Add {
    /// The day.
    pub date: Date,
    /// Start in minutes after midnight.
    pub start_minute: u16,
    /// Length in minutes (≤ 1440).
    pub duration_minutes: u16,
    /// Title (≤ [`MAX_TITLE`] bytes, not empty).
    pub title: String,
}

/// `calendar.remove` – removes an event.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Remove {
    /// Id of the event.
    pub id: u64,
}

/// How [`Add`] or [`Remove`] went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalendarOutcome {
    /// Done; the id of the event.
    Done(u64),
    /// A field is out of range.
    Invalid,
    /// No such event.
    Unknown,
    /// Nobody is in front.
    NoUser,
    /// The person's store is full or could not be written.
    Unavailable,
}

/// Answer to [`Add`] and [`Remove`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalendarReply {
    /// The result.
    pub outcome: CalendarOutcome,
}

/// `calendar.changed` – the events of the person in front changed (or the person did).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Changed {}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    Date,
    DayEvent,
    Day,
    DayReply,
    Add,
    Remove,
    CalendarReply,
    Changed
);

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(Day, "day", DayReply);
call!(Add, "add", CalendarReply);
call!(Remove, "remove", CalendarReply);

impl Topic for Changed {
    const TOPIC: &'static str = "calendar.changed";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let add = Add {
            date: Date {
                year: 2026,
                month: 11,
                day: 24,
            },
            start_minute: 9 * 60,
            duration_minutes: 30,
            title: "اجتماع".into(),
        };
        assert_eq!(decode::<Add>(&encode(&add).unwrap()).unwrap(), add);
        let reply = DayReply {
            events: vec![DayEvent {
                id: 1,
                start_minute: 540,
                duration_minutes: 30,
                title: "x".into(),
            }],
            more: false,
        };
        assert_eq!(decode::<DayReply>(&encode(&reply).unwrap()).unwrap(), reply);
        let r = CalendarReply {
            outcome: CalendarOutcome::Done(7),
        };
        assert_eq!(decode::<CalendarReply>(&encode(&r).unwrap()).unwrap(), r);
    }
}
