//! The phone shell on the bus: what the system services report goes into the [`Mobile`]
//! model, what the shell asks ([`Effect`]) goes out as one call each. Every call is best
//! effort: a missing service changes nothing and never stops the shell.
//!
//! The pure parts (apps from the launcher's list, a notification, a network state) are
//! functions of their own, tested without a bus.

pub use super::bus_map::{
    apply_net, apps_from, bars, launcher_id, next_of, note_from, shell_id, text_of,
};
use crate::app::Effect;
use crate::model::quick::Toggle;
use crate::model::Mobile;
use std::path::Path;
use std::time::Duration;
use ultar_bus::messages::{
    audio, bluetooth, calendar, launcher, lock, mesh, modem, net, notify, power, settings,
};
use ultar_bus::{Client, ClientOptions, TypedSubscription};
use ultar_i18n::Translator;

/// How long one call may take.
pub const CALL: Duration = Duration::from_secs(3);
/// The mesh may look for devices before it answers.
const MESH_CALL: Duration = Duration::from_secs(5);
/// Bus name of the shell.
pub const NAME: &str = "org.ultar.mobile";

/// Settings the quick settings switch directly.
const DND: &str = "notifications.do_not_disturb";
const NIGHT: &str = "display.night_light.enabled";
const LOCATION: &str = "privacy.location";
const POWER_MODE: &str = "power.mode";
const ZONE: &str = "datetime.timezone.name";

/// The topics the shell follows.
struct Topics {
    net: Option<TypedSubscription<net::NetState>>,
    bt: Option<TypedSubscription<bluetooth::BluetoothState>>,
    audio: Option<TypedSubscription<audio::AudioState>>,
    power: Option<TypedSubscription<power::PowerState>>,
    posted: Option<TypedSubscription<notify::Posted>>,
    dismissed: Option<TypedSubscription<notify::Dismissed>>,
    changed: Option<TypedSubscription<settings::Changed>>,
    calendar: Option<TypedSubscription<calendar::Changed>>,
    modem: Option<TypedSubscription<modem::ModemState>>,
    mesh: Option<TypedSubscription<mesh::MeshState>>,
    progress: Option<TypedSubscription<mesh::HandoffProgress>>,
    received: Option<TypedSubscription<ultar_bus::messages::mesh_received::FileReceived>>,
}

/// The shell's bus connection.
pub struct Link {
    client: Client,
    topics: Topics,
    /// `display.<id>.brightness` of the phone's panel, once the settings service named it.
    brightness: Option<String>,
    /// The time zone's name (`datetime.timezone.name`).
    zone: String,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").finish_non_exhaustive()
    }
}

impl Link {
    /// Connects and subscribes; `None` without a bus.
    pub fn connect(path: &Path) -> Option<Link> {
        let client = Client::connect(path, ClientOptions::named(NAME)).ok()?;
        let topics = Topics {
            net: client.subscribe().ok(),
            bt: client.subscribe().ok(),
            audio: client.subscribe().ok(),
            power: client.subscribe().ok(),
            posted: client.subscribe().ok(),
            dismissed: client.subscribe().ok(),
            changed: client.subscribe().ok(),
            calendar: client.subscribe().ok(),
            modem: client.subscribe().ok(),
            mesh: client.subscribe().ok(),
            progress: client.subscribe().ok(),
            received: client.subscribe().ok(),
        };
        Some(Link {
            client,
            topics,
            brightness: None,
            zone: super::tz::DEFAULT.into(),
        })
    }

    /// What is there at the start: apps, notifications, settings, today's next appointment.
    pub fn load(&mut self, m: &mut Mobile, tr: &Translator) {
        if let Ok(r) = self.client.call(&launcher::List {}, CALL) {
            let listed: Vec<String> = r.apps.into_iter().map(|a| a.app).collect();
            if !listed.is_empty() {
                m.launcher.apps = apps_from(&listed);
            }
        }
        if let Ok(r) = self.client.call(&notify::List {}, CALL) {
            let off = super::zone_offset(&self.zone);
            m.notes = r
                .notes
                .iter()
                .rev()
                .map(|p| note_from(p, tr, off))
                .collect();
        }
        for (key, t) in [
            (DND, Toggle::Dnd),
            (NIGHT, Toggle::NightLight),
            (LOCATION, Toggle::Location),
        ] {
            if let Some(settings::SettingValue::Flag(on)) = self.get(key) {
                m.quick.set(t, on);
            }
        }
        if let Some(settings::SettingValue::Text(zone)) = self.get(ZONE) {
            self.zone = zone;
        }
        if let Ok(d) = self.client.call(&modem::ModemSetData { on: None }, CALL) {
            m.quick.set(Toggle::MobileData, d.on);
        }
        if let Ok(t) = self.client.call(&power::Torch { on: None }, CALL) {
            m.hardware.torch = t.present;
            m.quick.set(Toggle::Torch, t.on);
        }
        if let Some(settings::SettingValue::Text(mode)) = self.get(POWER_MODE) {
            m.quick.set(Toggle::Saver, mode == "saver");
        }
        self.load_next(m);
    }

    /// Tells the settings service the phone's panel and reads its brightness.
    pub fn report_display(&mut self, m: &mut Mobile, outputs: &[ultar_surface::proto::OutputInfo]) {
        let displays: Vec<settings::DisplayDescriptor> = outputs
            .iter()
            .map(|o| settings::DisplayDescriptor {
                connector: o.name.to_ascii_lowercase(),
                internal: true,
                width_px: o.width,
                height_px: o.height,
                refresh_mhz: o.refresh_millihertz,
                ..settings::DisplayDescriptor::default()
            })
            .collect();
        let Ok(r) = self.client.call(&settings::SetDisplays { displays }, CALL) else {
            return;
        };
        self.brightness = r.ids.first().map(|id| format!("display.{id}.brightness"));
        if let Some(key) = self.brightness.clone() {
            if let Some(settings::SettingValue::Integer(v)) = self.get(&key) {
                m.quick.brightness = v.clamp(0, 100) as u8;
            }
        }
    }

    /// The time zone's name.
    pub fn zone(&self) -> &str {
        &self.zone
    }

    fn get(&self, key: &str) -> Option<settings::SettingValue> {
        let r = self
            .client
            .call(&settings::Get { key: key.into() }, CALL)
            .ok()?;
        r.value
    }

    fn load_next(&self, m: &mut Mobile) {
        let Some(now) = m.now else { return };
        let date = calendar::Date {
            year: u16::try_from(now.date.year()).unwrap_or(0),
            month: now.date.month(),
            day: now.date.day(),
        };
        if let Ok(r) = self.client.call(&calendar::Day { date }, CALL) {
            m.next = next_of(&r.events, u16::from(now.hour) * 60 + u16::from(now.minute));
        }
    }

    /// Takes what the services reported since the last call; `true` when the model changed.
    pub fn poll(&mut self, m: &mut Mobile, tr: &Translator) -> bool {
        let t = &self.topics;
        let mut changed = false;
        while let Some(n) = next(&t.net) {
            apply_net(m, &n);
            changed = true;
        }
        while let Some(b) = next(&t.bt) {
            m.hardware.bluetooth = b.present;
            m.quick.set(Toggle::Bluetooth, b.on);
            changed = true;
        }
        while let Some(a) = next(&t.audio) {
            m.quick.volume = a.volume_percent.min(100);
            changed = true;
        }
        while let Some(p) = next(&t.power) {
            m.status.battery = p.battery_percent;
            m.status.charging = p.charging;
            m.hardware.battery = p.battery_percent.is_some();
            changed = true;
        }
        while let Some(s) = next(&t.modem) {
            m.hardware.modem = s.cellular != ultar_bus::messages::call::Cellular::NoModem;
            changed = true;
        }
        while let Some(state) = next(&t.mesh) {
            m.hardware.mesh = true;
            m.mesh.state = Some(state);
            changed = true;
        }
        while let Some(p) = next(&t.progress) {
            m.mesh.progress(&p, m.now_ms);
            changed = true;
        }
        while let Some(f) = next(&t.received) {
            m.mesh.received(f.uri, m.now_ms);
            changed = true;
        }
        changed |= self.poll_notes(m, tr);
        changed |= self.poll_settings(m);
        if next(&self.topics.calendar).is_some() {
            self.load_next(m);
            changed = true;
        }
        changed
    }

    fn poll_notes(&self, m: &mut Mobile, tr: &Translator) -> bool {
        let mut changed = false;
        let off = super::zone_offset(&self.zone);
        while let Some(p) = next(&self.topics.posted) {
            m.notes.retain(|n| n.id != p.id);
            m.notes.insert(0, note_from(&p, tr, off));
            m.notes.truncate(MAX_NOTES);
            changed = true;
        }
        while let Some(d) = next(&self.topics.dismissed) {
            m.dismiss(d.id);
            changed = true;
        }
        changed
    }

    fn poll_settings(&mut self, m: &mut Mobile) -> bool {
        let mut changed = false;
        while let Some(c) = next(&self.topics.changed) {
            changed = true;
            match (c.key.as_str(), &c.value) {
                (DND, settings::SettingValue::Flag(on)) => m.quick.set(Toggle::Dnd, *on),
                (NIGHT, settings::SettingValue::Flag(on)) => m.quick.set(Toggle::NightLight, *on),
                (LOCATION, settings::SettingValue::Flag(on)) => m.quick.set(Toggle::Location, *on),
                (POWER_MODE, settings::SettingValue::Text(v)) => {
                    m.quick.set(Toggle::Saver, v == "saver");
                }
                (k, settings::SettingValue::Integer(v))
                    if Some(k) == self.brightness.as_deref() =>
                {
                    m.quick.brightness = (*v).clamp(0, 100) as u8;
                }
                _ => changed = false,
            }
        }
        changed
    }

    /// Carries out one effect (best effort).
    pub fn run(&self, effect: &Effect) {
        let c = &self.client;
        let _ = match effect {
            Effect::Launch(id) => self.start(id, None),
            Effect::Settings(page) => {
                let uri = page.map(|p| format!("settings://{p}"));
                self.start("org.ultar.settings", uri)
            }
            Effect::Close(id) => c
                .call(
                    &launcher::Stop {
                        app: launcher_id(id).into(),
                        role: launcher::Role::Ui,
                    },
                    CALL,
                )
                .map(|_| ()),
            Effect::Camera => self.start("org.ultar.camera", None),
            Effect::MeshCancel(offer) => c
                .call(&mesh::OfferCancel { offer: *offer }, MESH_CALL)
                .map(|_| ()),
            Effect::Open(uri) => self.start("org.ultar.files", Some(uri.clone())),
            Effect::Emergency => self.start("org.ultar.phone", Some("tel:".into())),
            Effect::Voice => self.start("org.ultar.assistant", None),
            Effect::Set(t, on) => self.switch(*t, *on),
            Effect::Brightness(v) => match &self.brightness {
                Some(key) => self.set(key, settings::SettingValue::Integer(i64::from(*v))),
                None => Ok(()),
            },
            Effect::Volume(v) => c.call(&audio::SetVolume { percent: *v }, CALL).map(|_| ()),
            Effect::Dismiss(id) => c.call(&notify::Dismiss { id: *id }, CALL).map(|_| ()),
            Effect::ClearNotes(ids) => {
                for id in ids {
                    let _ = c.call(&notify::Dismiss { id: *id }, CALL);
                }
                Ok(())
            }
            Effect::Restart => c.call(&power::Restart {}, CALL).map(|_| ()),
            Effect::PowerOff => c.call(&power::PowerOff {}, CALL).map(|_| ()),
            Effect::LockNow => c.call(&lock::Lock {}, CALL).map(|_| ()),
            // Sent by the shell itself (`send`): its answer changes the model.
            Effect::Unlock | Effect::MeshSend { .. } | Effect::Window(..) => Ok(()),
        };
    }

    /// Continue `app` (`title`: its name as shown) on `device`, or wherever it can go: the
    /// mesh is told what is in front, then asked to send it.
    pub fn send(
        &self,
        app: &str,
        title: &str,
        device: Option<String>,
    ) -> Option<mesh::SendOutcome> {
        let activity = mesh::Activity {
            app: app.to_owned(),
            title: title.to_owned(),
            uri: None,
            file: None,
        };
        let set = mesh::SetActivity {
            activity: Some(activity),
        };
        self.client.call(&set, MESH_CALL).ok()?;
        self.client
            .call(&mesh::HandoffSend { target: device }, MESH_CALL)
            .ok()
    }

    fn start(&self, app: &str, uri: Option<String>) -> Result<(), ultar_bus::BusError> {
        let start = launcher::Start {
            app: launcher_id(app).to_owned(),
            role: launcher::Role::Ui,
            activation_token: None,
            uri,
            new_window: false,
        };
        self.client.call(&start, CALL).map(|_| ())
    }

    fn set(&self, key: &str, value: settings::SettingValue) -> Result<(), ultar_bus::BusError> {
        let set = settings::Set {
            key: key.into(),
            value,
        };
        self.client.call(&set, CALL).map(|_| ())
    }

    fn switch(&self, t: Toggle, on: bool) -> Result<(), ultar_bus::BusError> {
        use settings::SettingValue::{Flag, Text};
        let c = &self.client;
        match t {
            Toggle::Wifi => c.call(&net::SetWifi { on }, CALL).map(|_| ()),
            Toggle::Airplane => c.call(&net::SetAirplane { on }, CALL).map(|_| ()),
            Toggle::Bluetooth => c.call(&bluetooth::Set { on }, CALL).map(|_| ()),
            Toggle::Dnd => self.set(DND, Flag(on)),
            Toggle::NightLight => self.set(NIGHT, Flag(on)),
            Toggle::Location => self.set(LOCATION, Flag(on)),
            Toggle::Saver => {
                let mode = if on { "saver" } else { "balanced" };
                self.set(POWER_MODE, Text(mode.into()))
            }
            Toggle::Vpn => c
                .call(&ultar_bus::messages::vpn::SetVpn { on, name: None }, CALL)
                .map(|_| ()),
            Toggle::MobileData => c
                .call(&modem::ModemSetData { on: Some(on) }, CALL)
                .map(|_| ()),
            Toggle::Torch => c.call(&power::Torch { on: Some(on) }, CALL).map(|_| ()),
            Toggle::Mesh => c.call(&mesh::SetEnabled { enabled: on }, CALL).map(|_| ()),
        }
    }
}

/// Notifications kept.
pub const MAX_NOTES: usize = 50;

fn next<T: ultar_bus::Topic>(sub: &Option<TypedSubscription<T>>) -> Option<T> {
    sub.as_ref().and_then(|s| s.try_recv().ok().flatten())
}
