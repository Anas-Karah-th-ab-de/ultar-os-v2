//! The update service follows the `updates.*` settings over a real bus: reads them at start,
//! maps them onto its policy under the administrator's ceiling, follows `settings.changed`,
//! notifies through `notifyd` (`updates.notify`) and offers an update found on a plugged-in
//! drive (`updates.offline_usb`). The settings/notify/volume services are fakes; the rest is real.

mod common;

use common::rig::*;
use common::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::notify::{Post, PostReply};
use ultar_bus::messages::settings::{Changed, Get, GetReply, SettingValue};
use ultar_bus::messages::volumes::{Mounted, VolumesState};
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_update::busfront::{serve, BusPublisher};
use ultar_update::contract::{GetPolicy, PolicyMessage};
use ultar_update::follow::{follow_settings, follow_volumes};
use ultar_update::notify::{BusPost, NotifyingPublisher, AVAILABLE_KEY, USB_KEY};
use ultar_update::settings::{Ceiling, UpdateSettings};

const UPDATED: &str = "system.updated";
const SETTINGSD: &str = "system.settingsd";
const NOTIFYD: &str = "system.notifyd";
const VOLUMED: &str = "system.volumed";

fn connect(dir: &Path, identity: &str) -> Client {
    Client::connect(&dir.join(identity), ClientOptions::named(identity)).unwrap()
}

/// A fake `settingsd`: answers `settings.get` from a map and publishes `settings.changed`.
struct Settings {
    values: Arc<Mutex<BTreeMap<String, SettingValue>>>,
    client: Client,
    _server: std::thread::JoinHandle<()>,
}

impl Settings {
    fn start(dir: &Path) -> Settings {
        let client = connect(dir, SETTINGSD);
        let values: Arc<Mutex<BTreeMap<String, SettingValue>>> = Arc::default();
        let endpoint = client.register("settings").unwrap();
        let served = values.clone();
        let server = std::thread::spawn(move || {
            while let Ok(request) = endpoint.recv() {
                if request.method() == "get" {
                    let map = served.lock().unwrap();
                    let _ = request.respond::<Get>(|call| {
                        Ok(GetReply {
                            value: map.get(&call.key).cloned(),
                        })
                    });
                } else {
                    let _ = request.fail(ultar_bus::ErrorCode::UnknownMethod);
                }
            }
        });
        Settings {
            values,
            client,
            _server: server,
        }
    }

    fn set(&self, key: &str, value: SettingValue) {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_string(), value.clone());
        let _ = self.client.publish(&Changed {
            key: key.to_string(),
            value,
        });
    }

    fn seed(&self, s: &UpdateSettings) {
        // Publish each as a change too, so the follower converges whether it reads the start
        // values first or is already following changes.
        for (key, value) in [
            ("updates.auto_system", SettingValue::Flag(s.auto_system)),
            (
                "updates.window.from",
                SettingValue::Text(time(s.window_from)),
            ),
            (
                "updates.window.until",
                SettingValue::Text(time(s.window_until)),
            ),
            ("updates.channel", SettingValue::Text(s.channel.clone())),
            ("updates.metered", SettingValue::Flag(s.metered)),
            ("updates.offline_usb", SettingValue::Flag(s.offline_usb)),
            ("updates.notify", SettingValue::Flag(s.notify)),
        ] {
            self.set(key, value);
        }
    }
}

fn time(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// A fake `notifyd` that collects the posts it accepts.
struct Notes {
    posts: Arc<Mutex<Vec<Post>>>,
    _client: Client,
    _server: std::thread::JoinHandle<()>,
}

impl Notes {
    fn start(dir: &Path) -> Notes {
        let client = connect(dir, NOTIFYD);
        let posts: Arc<Mutex<Vec<Post>>> = Arc::default();
        let endpoint = client.register("notify").unwrap();
        let collected = posts.clone();
        let server = std::thread::spawn(move || {
            let mut next = 0u64;
            while let Ok(request) = endpoint.recv() {
                if request.method() == "post" {
                    next += 1;
                    let id = next;
                    let sink = collected.clone();
                    let _ = request.respond::<Post>(move |post| {
                        sink.lock().unwrap().push(post);
                        Ok(PostReply { id })
                    });
                } else {
                    let _ = request.fail(ultar_bus::ErrorCode::UnknownMethod);
                }
            }
        });
        Notes {
            posts,
            _client: client,
            _server: server,
        }
    }

    fn titles(&self) -> Vec<String> {
        self.posts
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.title.key.clone())
            .collect()
    }
}

struct Rig2 {
    _broker: Broker,
    dir: tempfile::TempDir,
    device: Device,
    core: Arc<ultar_update::service::Core<ultar_update::filectl::FileBootControl>>,
    settings: Arc<Mutex<UpdateSettings>>,
    published: Arc<Mutex<Option<Vec<u8>>>>,
    service: Client,
    _server: std::thread::JoinHandle<()>,
}

fn rig2(ceiling: Ceiling) -> Rig2 {
    let dir = tempfile::tempdir().unwrap();
    let mut builder = Broker::builder(Policy::allow_all());
    for identity in [UPDATED, SETTINGSD, NOTIFYD, VOLUMED, ADMIN] {
        let auth = Arc::new(FixedIdentity::new(Identity::new(identity).unwrap()));
        builder = builder.listen(&dir.path().join(identity), auth);
    }
    let broker = builder.start().unwrap();
    let service = connect(dir.path(), UPDATED);
    let device = Device::new();
    let settings = Arc::new(Mutex::new(UpdateSettings::default()));
    let poster = Arc::new(BusPost(service.clone()));
    let publisher = Arc::new(NotifyingPublisher::new(
        BusPublisher(service.clone()),
        poster.clone(),
        settings.clone(),
    ));
    let published = Arc::new(Mutex::new(None));
    let (core, _cancel) = core_on(&device, Arc::new(Published(published.clone())), publisher);
    let endpoint = service.register("update").unwrap();
    let serving = core.clone();
    let server = std::thread::spawn(move || serve(serving, endpoint));
    follow_settings(core.clone(), service.clone(), ceiling, settings.clone());
    let roots = vec![device.dir.path().join("usb")];
    follow_volumes(service.clone(), roots, settings.clone(), poster);
    Rig2 {
        _broker: broker,
        dir,
        device,
        core,
        settings,
        published,
        service,
        _server: server,
    }
}

fn run_job(rig: &Rig2, job: ultar_update::service::Job) {
    rig.core.accept(ADMIN, &job).unwrap();
    rig.core.run(job).unwrap();
}

fn wait_titles(notes: &Notes, what: &str, ok: impl Fn(&[String]) -> bool) {
    let end = std::time::Instant::now() + WAIT;
    loop {
        let titles = notes.titles();
        if ok(&titles) {
            return;
        }
        assert!(std::time::Instant::now() < end, "{what}: {titles:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn policy_of(client: &Client) -> PolicyMessage {
    client.call(&GetPolicy {}, WAIT).unwrap()
}

fn wait_policy(client: &Client, what: &str, ok: impl Fn(&PolicyMessage) -> bool) {
    let end = std::time::Instant::now() + WAIT;
    loop {
        let p = policy_of(client);
        if ok(&p) {
            return;
        }
        assert!(std::time::Instant::now() < end, "{what}: {p:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn settings_reach_the_policy_at_start_and_on_change() {
    let rig = rig2(Ceiling::default());
    let settingsd = Settings::start(rig.dir.path());
    let admin = connect(rig.dir.path(), ADMIN);
    let person = UpdateSettings {
        auto_system: false,
        window_from: 60,
        window_until: 270,
        channel: "preview".into(),
        metered: true,
        ..UpdateSettings::default()
    };
    settingsd.seed(&person);
    // The rate limit is not on the page: it stays as the administrator set it.
    rig.core
        .set_policy(
            ADMIN,
            &PolicyMessage {
                rate_limit_kib: 300,
                ..policy_of(&rig.service)
            },
        )
        .unwrap();
    wait_policy(&admin, "start", |p| {
        !p.automatic && p.window == Some((60, 270)) && p.channel == "preview" && p.allow_metered
    });
    assert_eq!(policy_of(&admin).rate_limit_kib, 300);

    // A runtime change of a policy-relevant key is followed.
    settingsd.set("updates.auto_system", SettingValue::Flag(true));
    wait_policy(&admin, "auto on", |p| p.automatic);
    settingsd.set("updates.window.from", SettingValue::Text("23:00".into()));
    wait_policy(&admin, "window", |p| p.window == Some((1380, 270)));

    // An invalid value is ignored: the last good one stays.
    settingsd.set("updates.window.from", SettingValue::Text("99:99".into()));
    settingsd.set("updates.channel", SettingValue::Text("beta".into()));
    std::thread::sleep(Duration::from_millis(200));
    let p = policy_of(&admin);
    assert_eq!(p.window, Some((1380, 270)));
    assert_eq!(p.channel, "preview");

    // A key that only steers notifying does not touch the policy.
    settingsd.set("updates.notify", SettingValue::Flag(false));
    std::thread::sleep(Duration::from_millis(200));
    assert!(policy_of(&admin).automatic);
}

#[test]
fn the_ceiling_bounds_the_settings() {
    let ceiling = Ceiling {
        allow_automatic: false,
        allow_metered: false,
        channels: vec!["stable".into()],
    };
    let rig = rig2(ceiling);
    let settingsd = Settings::start(rig.dir.path());
    let admin = connect(rig.dir.path(), ADMIN);
    settingsd.seed(&UpdateSettings {
        auto_system: true,
        metered: true,
        channel: "preview".into(),
        ..UpdateSettings::default()
    });
    wait_policy(&admin, "ceiling", |p| {
        !p.automatic && !p.allow_metered && p.channel == "stable"
    });
    // Even turning the settings on cannot allow what the file forbids.
    settingsd.set("updates.auto_system", SettingValue::Flag(true));
    settingsd.set("updates.metered", SettingValue::Flag(true));
    std::thread::sleep(Duration::from_millis(200));
    let p = policy_of(&admin);
    assert!(!p.automatic && !p.allow_metered && p.channel == "stable");
}

#[test]
fn notifies_through_notifyd_while_switched_on() {
    use ultar_update::service::Job;
    let rig = rig2(Ceiling::default());
    let settingsd = Settings::start(rig.dir.path());
    settingsd.seed(&UpdateSettings::default());
    let notes = Notes::start(rig.dir.path());
    let new = release((0, 7, 1), 2);
    *rig.published.lock().unwrap() = Some(new.bundle.clone());

    // A found release is posted once through notifyd.
    run_job(&rig, Job::Check);
    wait_titles(&notes, "available", |t| {
        t.iter().any(|k| k == AVAILABLE_KEY)
    });
    run_job(
        &rig,
        Job::Download {
            allow_metered: true,
        },
    );
    assert_eq!(
        notes
            .titles()
            .iter()
            .filter(|t| *t == AVAILABLE_KEY)
            .count(),
        1
    );

    // Switched off at runtime: the restart notice is not posted.
    settingsd.set("updates.notify", SettingValue::Flag(false));
    let end = std::time::Instant::now() + WAIT;
    while rig.settings.lock().unwrap().notify {
        assert!(std::time::Instant::now() < end, "notify off not followed");
        std::thread::sleep(Duration::from_millis(20));
    }
    run_job(&rig, Job::Apply);
    std::thread::sleep(Duration::from_millis(300));
    let titles = notes.titles();
    assert_eq!(
        titles,
        [AVAILABLE_KEY],
        "restart notice posted while off: {titles:?}"
    );
}

#[test]
fn offers_an_update_from_a_plugged_in_drive() {
    let rig = rig2(Ceiling::default());
    let settingsd = Settings::start(rig.dir.path());
    settingsd.seed(&UpdateSettings::default());
    let notes = Notes::start(rig.dir.path());
    let volumed = connect(rig.dir.path(), VOLUMED);
    // A bundle on the stick (inside the allowed root).
    let new = release((0, 7, 1), 2);
    rig.device.write_bundle("update.uup", &new);
    let stick = rig.device.dir.path().join("usb");
    let mounted = VolumesState {
        mounted: vec![Mounted {
            id: "sdb1".into(),
            name: "STICK".into(),
            uri: format!("file://{}", stick.display()),
        }],
    };
    volumed.publish(&mounted).unwrap();
    wait_titles(&notes, "drive offer", |t| t.iter().any(|k| k == USB_KEY));
    // The same drive is not offered again on a later state event.
    volumed.publish(&mounted).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(notes.titles().iter().filter(|t| *t == USB_KEY).count(), 1);

    // Switched off: a newly plugged drive is not offered.
    settingsd.set("updates.offline_usb", SettingValue::Flag(false));
    let end = std::time::Instant::now() + WAIT;
    while rig.settings.lock().unwrap().offline_usb {
        assert!(
            std::time::Instant::now() < end,
            "offline_usb off not followed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let other = VolumesState {
        mounted: vec![Mounted {
            id: "sdc1".into(),
            name: "OTHER".into(),
            uri: format!("file://{}", stick.display()),
        }],
    };
    volumed.publish(&other).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(notes.titles().iter().filter(|t| *t == USB_KEY).count(), 1);
}
