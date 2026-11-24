//! The retained topics of the system services the shell follows (docs/integration/
//! system-services.md): each report goes into [`Services`](crate::model::services::Services);
//! the presence flags of the quick settings follow from them.

use crate::model::Desktop;
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::calendar::Changed as CalendarChanged;
use ultar_bus::messages::media::MediaState;
use ultar_bus::messages::net::{NetState, WifiLink};
use ultar_bus::messages::notify::Dismissed;
use ultar_bus::messages::power::PowerActions;
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::volumes::VolumesState;
use ultar_bus::messages::vpn::VpnState;
use ultar_bus::{Client, Topic, TypedSubscription};

/// Subscriptions of the service topics (each `None` when the bus refused it).
pub struct ServiceTopics {
    power: Option<TypedSubscription<PowerActions>>,
    session: Option<TypedSubscription<SessionState>>,
    audio: Option<TypedSubscription<AudioState>>,
    net: Option<TypedSubscription<NetState>>,
    bluetooth: Option<TypedSubscription<BluetoothState>>,
    volumes: Option<TypedSubscription<VolumesState>>,
    media: Option<TypedSubscription<MediaState>>,
    dismissed: Option<TypedSubscription<Dismissed>>,
    calendar: Option<TypedSubscription<CalendarChanged>>,
    wifi: Option<TypedSubscription<WifiLink>>,
    vpn: Option<TypedSubscription<VpnState>>,
}

/// Takes every waiting message of `sub`; `true` when there was one.
fn drain<T: Topic>(sub: &Option<TypedSubscription<T>>, mut take: impl FnMut(T)) -> bool {
    let mut any = false;
    while let Some(Ok(Some(m))) = sub.as_ref().map(|s| s.try_recv()) {
        take(m);
        any = true;
    }
    any
}

impl ServiceTopics {
    /// Subscribes to every topic.
    pub fn subscribe(client: &Client) -> ServiceTopics {
        ServiceTopics {
            power: client.subscribe().ok(),
            session: client.subscribe().ok(),
            audio: client.subscribe().ok(),
            net: client.subscribe().ok(),
            bluetooth: client.subscribe().ok(),
            volumes: client.subscribe().ok(),
            media: client.subscribe().ok(),
            dismissed: client.subscribe().ok(),
            calendar: client.subscribe().ok(),
            wifi: client.subscribe().ok(),
            vpn: client.subscribe().ok(),
        }
    }

    /// Takes what arrived; returns whether anything changed and whether the shown calendar
    /// day must be asked for again (`calendar.changed`).
    pub fn poll(&self, d: &mut Desktop) -> (bool, bool) {
        let svc = &mut d.svc;
        let mut any = drain(&self.power, |m| svc.power = Some(m));
        any |= drain(&self.session, |m| svc.session = Some(m));
        any |= drain(&self.audio, |m| svc.audio = Some(m));
        any |= drain(&self.net, |m| svc.net = Some(m));
        any |= drain(&self.bluetooth, |m| svc.bluetooth = Some(m));
        any |= drain(&self.volumes, |m| svc.set_drives(m.mounted));
        any |= drain(&self.media, |m| svc.media = m.now);
        any |= drain(&self.wifi, |m| svc.wifi.link = Some(m));
        any |= drain(&self.vpn, |m| svc.vpn = Some(m));
        let notes = &mut d.notes;
        any |= drain(&self.dismissed, |m| {
            notes.dismiss(m.id);
        });
        let calendar = drain(&self.calendar, |_| {});
        follow_presence(d);
        (any || calendar, calendar)
    }
}

/// The quick settings' presence flags from the reports: the network tiles need `net.state`,
/// the sound controls an audio device with a volume.
pub fn follow_presence(d: &mut Desktop) {
    d.presence.net_service = d.svc.net.is_some();
    d.presence.audio_service = d
        .svc
        .audio
        .as_ref()
        .is_some_and(|a| a.present && a.has_volume);
}
