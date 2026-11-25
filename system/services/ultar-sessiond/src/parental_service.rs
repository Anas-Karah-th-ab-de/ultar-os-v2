//! The parental window on the bus side: read `users.parental*` (at start and on
//! `settings.changed`), warn standard users shortly before the window ends, end their
//! sessions at its end. Split from [`crate::service`] so that file stays the calls.

use crate::parental::{self, Due};
use crate::service::{accounts, lock, SessionService, WAIT};
use ultar_bus::messages::notify::{Body, Category, Post, Urgency};
use ultar_bus::messages::settings::{Get, SettingValue};
use ultar_bus::{Text, TextArg};
use ultar_service::log::{Level, Record};
use ultar_svckit::serve::Done;
use ultar_svckit::Bus;

impl SessionService {
    /// Reads the parental settings (at start, and again until the settings service
    /// answered). Until then nothing is enforced – never on a guess.
    pub(crate) fn read_settings(&mut self, bus: &Bus<'_, Self>) {
        if self.settings_known {
            return;
        }
        let _ = bus.later(|client| {
            let mut answered = false;
            let mut values: Vec<(String, SettingValue)> = Vec::new();
            for key in parental::KEYS {
                let get = Get {
                    key: key.to_owned(),
                };
                if let Ok(reply) = client.call(&get, WAIT) {
                    answered = true;
                    values.extend(reply.value.map(|v| (key.to_owned(), v)));
                }
            }
            Box::new(move |s: &mut Self, _bus: &Bus<'_, Self>| {
                s.settings_known |= answered;
                for (key, value) in &values {
                    if s.parental.on_changed(key, value) {
                        s.warned.clear();
                    }
                }
            })
        });
    }

    /// The parental window: warn standard users shortly before it ends, end their
    /// sessions at its end (the screen is locked first when one of them is in front).
    pub(crate) fn enforce_parental(&mut self, bus: &Bus<'_, Self>) {
        if !self.parental.on || self.sessions.signed_in().is_empty() {
            return;
        }
        let due = self.parental.due(parental::minute_now());
        if due == Due::Nothing {
            return;
        }
        let signed_in = self.sessions.signed_in().to_vec();
        let active = self.sessions.active().map(str::to_owned);
        let warned = self.warned.clone();
        let until = self.parental.until_min;
        let _ = bus.later(move |client| {
            let kinds = accounts(client);
            let children = standard_users(&signed_in, kinds.as_deref());
            // The screen must cover the desktop before the session in front ends.
            let locked = due != Due::End
                || !active.as_ref().is_some_and(|a| children.contains(a))
                || lock(client);
            let known = kinds.is_some();
            Box::new(move |s: &mut Self, bus: &Bus<'_, Self>| {
                if known {
                    s.parental_done(bus, due, &children, &warned, until, locked);
                }
            })
        });
    }

    /// Warns or ends the sessions of the standard users (on the loop).
    fn parental_done(
        &mut self,
        bus: &Bus<'_, Self>,
        due: Due,
        children: &[String],
        warned: &[String],
        until: u16,
        locked: bool,
    ) {
        match due {
            Due::Warn => {
                for name in children {
                    if !warned.contains(name) && !self.warned.contains(name) {
                        warn_child(bus, name, until);
                        self.warned.push(name.clone());
                    }
                }
            }
            Due::End if locked => {
                let mut ended = false;
                for name in children {
                    ended |= self.sessions.end(name);
                }
                if ended {
                    self.warned.retain(|w| !children.contains(w));
                    self.changed(bus);
                    bus.log(Record::new(Level::Info, "sessiond.parental_end"));
                }
            }
            _ => {}
        }
    }
}

/// The signed-in standard users (administrators are never restricted).
fn standard_users(signed_in: &[String], kinds: Option<&[(String, String, bool)]>) -> Vec<String> {
    let Some(kinds) = kinds else {
        return Vec::new();
    };
    signed_in
        .iter()
        .filter(|n| kinds.iter().any(|(k, _, admin)| k == *n && !admin))
        .cloned()
        .collect()
}

/// The warning notification (the shell shows it; the texts are in its catalogs).
fn warn_child(bus: &Bus<'_, SessionService>, name: &str, until_min: u16) {
    let until = format!("{:02}:{:02}", until_min / 60, until_min % 60);
    let post = Post {
        urgency: Urgency::Normal,
        category: Category::System,
        app_id: "org.ultar.session".into(),
        title: Text::key("sessiond.parental.title"),
        body: Body::Localized(
            Text::key("sessiond.parental.body")
                .with(TextArg::Verbatim(name.to_owned()))
                .with(TextArg::Verbatim(until)),
        ),
        actions: Vec::new(),
    };
    let _ = bus.later(move |client| {
        let _ = client.call(&post, WAIT);
        let done: Done<SessionService> =
            Box::new(|_: &mut SessionService, _: &Bus<'_, SessionService>| {});
        done
    });
}
