//! Following the Storage page settings: read at start (`settings.get`, again until the
//! settings service answered) and at runtime (`settings.changed`), and the periodic work
//! they drive – the low-space warning and the daily downloads clean-up – plus the "Mount"
//! button of a drive offer.

use ultar_bus::messages::notify::TakeInvoked;
use ultar_bus::messages::settings::{Get, SettingValue};
use ultar_service::log::{Level, Record};
use ultar_svckit::Bus;

use crate::clean::clean_downloads;
use crate::jobs::{self, MOUNT_ACTION};
use crate::service::{now, space_of, VolumeService, CALL_TIME};
use crate::settings::{AUTOMOUNT, KEYS};

impl VolumeService {
    /// Reads the Storage settings (at start, and again until the settings service
    /// answered). Until then the schema defaults are used.
    pub(crate) fn read_settings(&self, bus: &Bus<'_, Self>) {
        if self.settings_known {
            return;
        }
        let _ = bus.later(|client| {
            let mut answered = false;
            let mut values: Vec<(String, SettingValue)> = Vec::new();
            for key in KEYS {
                if let Ok(reply) = client.call(
                    &Get {
                        key: key.to_owned(),
                    },
                    CALL_TIME,
                ) {
                    answered = true;
                    values.extend(reply.value.map(|value| (key.to_owned(), value)));
                }
            }
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                service.settings_known |= answered;
                for (key, value) in &values {
                    service.apply_setting(key, value, bus);
                }
            })
        });
    }

    /// One Storage setting has this value now.
    pub(crate) fn apply_setting(&mut self, key: &str, value: &SettingValue, bus: &Bus<'_, Self>) {
        let automount_was = self.settings.automount;
        if !self.settings.on_changed(key, value) {
            return;
        }
        if key == AUTOMOUNT && self.settings.automount && !automount_was {
            // Switched on: the offered drives mount by themselves again.
            self.clear_offers(bus);
            self.scan(bus);
        }
    }

    /// The periodic work: warn once when the data partition runs low, and clean up the
    /// downloads folder once a day while that is switched on.
    pub(crate) fn run_jobs(&mut self, bus: &Bus<'_, Self>) {
        if let Some((total, free)) = space_of(&self.internal()) {
            if let Some(percent) = self
                .jobs
                .low_space(total, free, self.settings.low_space_percent)
            {
                let post = jobs::low_space_post(percent);
                let _ = bus.later(move |client| {
                    let _ = client.call(&post, CALL_TIME);
                    Box::new(|_, _| {})
                });
            }
        }
        let now = now();
        if self.jobs.clean_due(self.settings.clean_downloads, now) {
            if let Some(uid) = self.user.as_ref().and_then(|u| self.uids.get(u).copied()) {
                let days = self.settings.clean_downloads_days;
                let removed = clean_downloads(&self.downloads, uid, days, now);
                self.jobs.cleaned(now);
                if removed > 0 {
                    bus.log(
                        Record::new(Level::Info, "volumed.cleaned").count("files", removed as u64),
                    );
                }
            }
        }
    }

    /// A notification button was pressed: mount the offered drive it names.
    pub(crate) fn invoked(&mut self, bus: &Bus<'_, Self>) {
        let _ = bus.later(|client| {
            let invoked = client
                .call(&TakeInvoked {}, CALL_TIME)
                .map(|r| r.invoked)
                .unwrap_or_default();
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                for action in invoked {
                    if action.action != MOUNT_ACTION {
                        continue;
                    }
                    let Some(offer) = service
                        .offered
                        .values()
                        .find(|o| o.note == action.id)
                        .cloned()
                    else {
                        continue;
                    };
                    service.offered.remove(&offer.volume.device);
                    service.mount_one(&offer.volume, bus);
                    service.publish(bus);
                }
            })
        });
    }
}
