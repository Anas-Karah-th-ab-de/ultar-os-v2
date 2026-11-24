//! The bus side of the calendar service.

use crate::book::Book;
use std::path::PathBuf;
use ultar_bus::messages::calendar::{
    Add, CalendarOutcome, CalendarReply, Changed, Day, DayReply, Remove, SERVICE,
};
use ultar_bus::messages::session::SessionState;
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_service::log::{Level, Record};
use ultar_svckit::store::{self, StoreError};
use ultar_svckit::{Bus, Service};

/// Longest account name used as a file name.
const MAX_NAME: usize = 32;

/// The calendar service.
pub struct CalendarService {
    dir: PathBuf,
    user: Option<String>,
    /// The book of `user`; `Err` while its file is damaged or unreadable.
    book: Result<Book, ()>,
}

/// An account name that is safe as a file name.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl CalendarService {
    /// Books are kept below `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            user: None,
            book: Ok(Book::default()),
        }
    }

    fn path(&self, user: &str) -> PathBuf {
        self.dir.join(format!("{user}.cal"))
    }

    fn switch_to(&mut self, user: Option<String>, bus: &Bus<'_, Self>) {
        let user = user.filter(|u| safe_name(u));
        if user == self.user {
            return;
        }
        self.book = match &user {
            None => Ok(Book::default()),
            Some(name) => match store::load::<Book>(&self.path(name)) {
                Ok(book) if book.is_valid() => Ok(book),
                Err(StoreError::Missing) => Ok(Book::default()),
                _ => {
                    bus.log(Record::new(Level::Error, "calendard.book_damaged"));
                    Err(())
                }
            },
        };
        self.user = user;
        bus.publish(&Changed {});
    }

    fn change(
        &mut self,
        bus: &Bus<'_, Self>,
        edit: impl FnOnce(&mut Book) -> CalendarOutcome,
    ) -> CalendarOutcome {
        let Some(user) = self.user.clone() else {
            return CalendarOutcome::NoUser;
        };
        let Ok(book) = &self.book else {
            return CalendarOutcome::Unavailable;
        };
        let mut next = book.clone();
        let outcome = edit(&mut next);
        if !matches!(outcome, CalendarOutcome::Done(_)) {
            return outcome;
        }
        if store::save(&self.path(&user), &next).is_err() {
            bus.log(Record::new(Level::Error, "calendard.not_saved"));
            return CalendarOutcome::Unavailable;
        }
        self.book = Ok(next);
        bus.publish(&Changed {});
        outcome
    }
}

impl Service for CalendarService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![SessionState::TOPIC]
    }

    fn start(&mut self, _bus: &Bus<'_, Self>) {}

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        match request.method() {
            m if m == Day::METHOD => {
                let reply = match (&self.user, &self.book) {
                    (Some(_), Ok(book)) => request.decode::<Day>().map(|d| book.day(d.date)),
                    _ => request.decode::<Day>().map(|_| DayReply::default()),
                };
                let _ = match reply {
                    Ok(r) => request.reply(&r),
                    Err(_) => request.fail(ErrorCode::BadPayload),
                };
            }
            m if m == Add::METHOD => match request.decode::<Add>() {
                Ok(add) => {
                    let outcome = self.change(bus, |b| b.add(add));
                    let _ = request.reply(&CalendarReply { outcome });
                }
                Err(_) => {
                    let _ = request.fail(ErrorCode::BadPayload);
                }
            },
            m if m == Remove::METHOD => match request.decode::<Remove>() {
                Ok(remove) => {
                    let outcome = self.change(bus, |b| b.remove(remove.id));
                    let _ = request.reply(&CalendarReply { outcome });
                }
                Err(_) => {
                    let _ = request.fail(ErrorCode::BadPayload);
                }
            },
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
            }
        }
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if let Ok(state) = event.decode::<SessionState>() {
            self.switch_to(state.user, bus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_names_become_file_names() {
        assert!(safe_name("amal") && safe_name("a_b-1"));
        for bad in ["", "-x", "../x", "a/b", "Amal", &"x".repeat(33)] {
            assert!(!safe_name(bad), "{bad}");
        }
    }
}
