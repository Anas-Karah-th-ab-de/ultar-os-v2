//! With `storage.automount` off, a removable drive is offered with a "Mount" button
//! instead of mounted by itself. The offer is a notification ([`crate::jobs::drive_post`]);
//! its id is kept so the offer can be withdrawn when the drive is pulled or automount is
//! switched on again, and found when the person presses the button ([`crate::follow`]).

use ultar_bus::messages::notify::Dismiss;
use ultar_hal_linux::block::Volume;
use ultar_svckit::Bus;

use crate::jobs;
use crate::service::{VolumeService, CALL_TIME};

/// A drive offered instead of mounted: its volume and its notification (0 while the
/// notification is still on its way to the notification service).
#[derive(Clone, Debug)]
pub(crate) struct Offer {
    pub volume: Volume,
    pub note: u64,
}

impl Offer {
    /// Withdraws the offer notification (the drive was pulled, or automount switched on).
    pub fn withdraw(&self, bus: &Bus<'_, VolumeService>) {
        if self.note != 0 {
            let id = self.note;
            let _ = bus.later(move |client| {
                let _ = client.call(&Dismiss { id }, CALL_TIME);
                Box::new(|_, _| {})
            });
        }
    }
}

impl VolumeService {
    /// Offers a drive with a "Mount" button instead of mounting it. The drive is marked
    /// offered at once so a second tick does not offer it twice; the notification id
    /// arrives with the answer (a refusal leaves it to be offered again).
    pub(crate) fn offer(&mut self, volume: &Volume, bus: &Bus<'_, Self>) {
        let device = volume.device.clone();
        let name = if volume.label.is_empty() {
            device.clone()
        } else {
            volume.label.clone()
        };
        self.offered.insert(
            device.clone(),
            Offer {
                volume: volume.clone(),
                note: 0,
            },
        );
        let post = jobs::drive_post(&name);
        let dev = device.clone();
        let started = bus.later(move |client| {
            let note = client.call(&post, CALL_TIME).ok().map(|r| r.id);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                service.note_arrived(&dev, note, bus);
            })
        });
        if !started {
            self.offered.remove(&device);
        }
    }

    /// The offer notification was posted (or not): keep its id, or withdraw it when the
    /// drive was pulled while the answer was on its way.
    pub(crate) fn note_arrived(&mut self, device: &str, note: Option<u64>, bus: &Bus<'_, Self>) {
        match (note, self.offered.get_mut(device)) {
            (Some(id), Some(offer)) => offer.note = id,
            (Some(id), None) => {
                let _ = bus.later(move |client| {
                    let _ = client.call(&Dismiss { id }, CALL_TIME);
                    Box::new(|_, _| {})
                });
            }
            (None, _) => {
                self.offered.remove(device);
            }
        }
    }

    /// Withdraws every drive offer (automount switched on).
    pub(crate) fn clear_offers(&mut self, bus: &Bus<'_, Self>) {
        let offers: Vec<Offer> = self.offered.values().cloned().collect();
        self.offered.clear();
        for offer in offers {
            offer.withdraw(bus);
        }
    }
}
