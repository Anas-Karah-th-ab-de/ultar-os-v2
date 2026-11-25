//! Every setting of the new pages that takes effect, changed through its page: the value
//! reaches the settings service, the service that follows it is told (`settings.changed`,
//! received with that service's own identity) and its own code turns the change into the
//! effect – the display service into the compositor request, the shell into what its search
//! looks through and how its surfaces look; the Settings window follows the accessibility
//! switches itself. The Storage page shows what the volume service measured.

use super::{click, open, setup::Run};
use std::time::Duration;
use ultar_bus::messages::settings::{Changed, Get, SettingValue};
use ultar_bus::{Client, TypedSubscription};
use ultar_desktop::app::desktop::link::apply_setting;
use ultar_desktop::model::search::{search_in, Hit};
use ultar_desktop::model::Desktop;
use ultar_displayd::input::InputSettings;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::areas::index_of;
use ultar_settings::App;
use ultar_surface::proto::Request;
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::Size;

#[path = "storage.rs"]
mod storage;

const WAIT: Duration = Duration::from_secs(20);

/// A service that follows the settings: its connection and what it was told.
struct Follower {
    name: &'static str,
    changes: TypedSubscription<Changed>,
    _client: Client,
}

impl Follower {
    fn connect(run: &Run, name: &'static str, socket: &str) -> Result<Follower, String> {
        let client = run.client_at(socket)?;
        let changes = client
            .subscribe::<Changed>()
            .map_err(|e| format!("{name}: subscribe {e}"))?;
        Ok(Follower {
            name,
            changes,
            _client: client,
        })
    }

    /// Waits until this service is told that `key` is now `value`.
    fn told(&self, key: &str, value: &SettingValue) -> Result<(), String> {
        loop {
            let change = self
                .changes
                .recv_timeout(WAIT)
                .map_err(|e| format!("{} was not told about {key}: {e}", self.name))?;
            if change.key == key {
                return (change.value == *value)
                    .then_some(())
                    .ok_or(format!("{} was told {key}={:?}", self.name, change.value));
            }
        }
    }
}

/// The node of the row of `key` on the open page, which must be usable.
fn control(app: &mut App, panel: &str, key: &str) -> Result<ultar_ui::ui::NodeId, String> {
    let items = &app
        .state
        .panel(panel)
        .ok_or(format!("panel {panel}"))?
        .items;
    let row = items
        .iter()
        .position(|i| i.key == key)
        .ok_or(format!("{key}: no row"))?;
    let node = app
        .view
        .node(Act::PanelItem(row))
        .ok_or(format!("{key}: no control"))?;
    if app.ui.is_disabled(node) {
        return Err(format!("{key}: the control is disabled"));
    }
    Ok(node)
}

/// Clicks the switch of `key`, or its slider at `at` of its width; the value the settings
/// service holds afterwards.
fn change(
    (app, admin): (&mut App, &Client),
    (panel, key): (&str, &str),
    at: Option<f32>,
) -> Result<SettingValue, String> {
    let node = control(app, panel, key)?;
    match at {
        None => click(app, node)?,
        Some(share) => {
            app.ui.layout();
            let rect = app.ui.rect(node).ok_or("no rect")?;
            let mut pos = rect.center();
            pos.x = rect.x + rect.w * share;
            let button = PointerButton::Primary;
            for event in [
                Event::PointerMove { pos },
                Event::PointerDown { pos, button },
                Event::PointerUp { pos, button },
            ] {
                app.handle(event).map_err(|e| format!("{e:?}"))?;
            }
        }
    }
    let get = Get { key: key.into() };
    let reply = admin.call(&get, WAIT).map_err(|e| format!("get {e}"))?;
    reply.value.ok_or(format!("{key}: no value"))
}

fn page(bus: &Client, run: &Run, area: &str) -> Result<App, String> {
    let area = index_of(area).ok_or(format!("area {area}"))?;
    open(bus, run, Lang::En, Size::new(1280.0, 800.0), area)
}

/// What the display service does with a change it is told about.
struct Display<'a> {
    told: &'a Follower,
    service: InputSettings,
}

impl Display<'_> {
    fn follow(&mut self, key: &str, value: SettingValue) -> Result<(), String> {
        self.told.told(key, &value)?;
        self.service
            .on_changed(key, &value)
            .then_some(())
            .ok_or(format!("the display service ignores {key}={value:?}"))
    }
}

fn whole(key: &str, value: &SettingValue) -> Result<i64, String> {
    match value {
        SettingValue::Integer(n) => Ok(*n),
        other => Err(format!("{key} is {other:?}")),
    }
}

/// Mouse page: faster pointer, natural scrolling. Returns the speed set.
fn pointer(bus: &Client, admin: &Client, run: &Run, d: &mut Display<'_>) -> Result<i64, String> {
    let mut app = page(bus, run, "mouse")?;
    let natural = change(
        (&mut app, admin),
        ("pointer", "pointer.natural_scroll"),
        None,
    )?;
    d.follow("pointer.natural_scroll", natural)?;
    let speed = change((&mut app, admin), ("pointer", "pointer.speed"), Some(0.9))?;
    let fast = whole("pointer.speed", &speed)?;
    d.follow("pointer.speed", speed)?;
    Ok(fast)
}

/// Keyboard page: longer delay, slower repeats, then no repeats. Returns delay and rate.
fn repeat(
    bus: &Client,
    admin: &Client,
    run: &Run,
    d: &mut Display<'_>,
) -> Result<(i64, i64), String> {
    let mut app = page(bus, run, "keyboard")?;
    let mut slide = |key: &str, share: f32| {
        let value = change((&mut app, admin), ("keyboard", key), Some(share))?;
        let n = whole(key, &value)?;
        d.follow(key, value)?;
        Ok::<i64, String>(n)
    };
    let delay_ms = slide("keyboard.repeat.delay_ms", 0.9)?;
    let per_second = slide("keyboard.repeat.per_second", 0.1)?;
    let key = "keyboard.repeat.enabled";
    let off = change((&mut app, admin), ("keyboard", key), None)?;
    d.follow(key, off)?;
    Ok((delay_ms, per_second))
}

/// Mouse and keyboard: the display service turns the settings into compositor requests.
fn input(bus: &Client, admin: &Client, run: &Run, displayd: &Follower) -> Result<(), String> {
    let mut d = Display {
        told: displayd,
        service: InputSettings::default(),
    };
    let fast = pointer(bus, admin, run, &mut d)?;
    let (delay_ms, per_second) = repeat(bus, admin, run, &mut d)?;
    let wanted = [
        Request::ConfigurePointer {
            speed: i8::try_from(fast).map_err(|_| "speed")?,
            natural_scroll: true,
        },
        Request::ConfigureKeyRepeat {
            enabled: false,
            delay_ms: u16::try_from(delay_ms).map_err(|_| "delay")?,
            per_second: u8::try_from(per_second).map_err(|_| "rate")?,
        },
    ];
    let requests = d.service.requests();
    if fast <= 0 || delay_ms <= 500 || per_second >= 25 || requests != wanted {
        return Err(format!("compositor requests {requests:?}"));
    }
    println!(
        "SETTINGS: mouse and keyboard pages -> display service -> compositor: speed {fast}, \
         natural scrolling, repeat off ({delay_ms} ms, {per_second}/s)"
    );
    Ok(())
}

/// Search and accessibility: the shell's own state follows, and the Settings window too.
fn shell(bus: &Client, admin: &Client, run: &Run, shell: &Follower) -> Result<(), String> {
    let mut desk = Desktop::new(Lang::En);
    let follow = |desk: &mut Desktop, key: &str, value: SettingValue| {
        shell.told(key, &value)?;
        apply_setting(desk, key, &value);
        Ok::<(), String>(())
    };
    let tr = ultar_desktop::i18n::translator(Lang::En).map_err(|e| format!("{e:?}"))?;
    let apps = ultar_desktop::model::apps::base_apps();
    let finds_files = |desk: &Desktop| {
        search_in("files", &apps, &tr, desk.overview.sources)
            .iter()
            .any(|hit| matches!(hit, Hit::App(_)))
    };
    if !finds_files(&desk) {
        return Err("the shell's search does not find the Files app".into());
    }
    let mut app = page(bus, run, "search")?;
    for key in ["search.source.apps", "search.source.calculator"] {
        let off = change((&mut app, admin), ("search", key), None)?;
        follow(&mut desk, key, off)?;
    }
    let sources = desk.overview.sources;
    if finds_files(&desk) || sources.apps || sources.calculator || !sources.settings {
        return Err(format!("the shell still searches {sources:?}"));
    }
    let mut app = page(bus, run, "accessibility")?;
    for key in [
        "accessibility.large_text",
        "accessibility.high_contrast",
        "accessibility.reduce_motion",
    ] {
        let on = change((&mut app, admin), ("accessibility", key), None)?;
        follow(&mut desk, key, on)?;
    }
    let v = desk.values;
    if !(v.large_text && v.high_contrast && v.reduce_motion) {
        return Err(format!("the shell's look is {v:?}"));
    }
    let env = app.ui.env();
    if !(env.high_contrast && env.text_scale > 1.0) {
        return Err("the Settings window did not follow the accessibility switches".into());
    }
    // Back to the usual look for whoever runs after this.
    for key in [
        "accessibility.large_text",
        "accessibility.high_contrast",
        "accessibility.reduce_motion",
    ] {
        change((&mut app, admin), ("accessibility", key), None)?;
    }
    println!(
        "SETTINGS: search and accessibility pages -> shell: apps and calculator not searched, \
         large text, high contrast, no motion (the Settings window too)"
    );
    Ok(())
}

/// Everything above.
pub fn wired(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let displayd = Follower::connect(run, "the display service", "displayd")?;
    let desktop = Follower::connect(run, "the shell", "desktop")?;
    let volumed = Follower::connect(run, "the volume service", "volumed")?;
    input(bus, admin, run, &displayd)?;
    shell(bus, admin, run, &desktop)?;
    storage::storage(bus, run)?;
    storage::storage_settings(bus, admin, run, &volumed)
}
