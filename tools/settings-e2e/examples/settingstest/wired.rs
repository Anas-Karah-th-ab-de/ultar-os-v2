//! Every setting of the new pages that takes effect, changed through its page: the value
//! reaches the settings service, the service that follows it is told (`settings.changed`,
//! received with that service's own identity) and its own code turns the change into the
//! effect – the display service into the compositor request, the shell into what its search
//! looks through and how its surfaces look; the Settings window follows the accessibility
//! switches itself. The Storage page shows what the volume service measured.

use super::{click, open, setup::Run, texts};
use std::time::Duration;
use ultar_bus::messages::settings::{Changed, Get, Set, SettingValue};
use ultar_bus::{Client, TypedSubscription};
use ultar_desktop::app::desktop::link::apply_setting;
use ultar_desktop::model::history::Record;
use ultar_desktop::model::search::{search_in, search_in_with_history, Hit};
use ultar_desktop::model::Desktop;
use ultar_displayd::input::InputSettings;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::areas::index_of;
use ultar_settings::App;
use ultar_surface::proto::Request;
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::Size;
use ultar_update::policy::Policy;
use ultar_update::settings::{Ceiling, UpdateSettings};

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

/// Characters and history: the shell's own tables follow their switches. A name offers the
/// character; switching the source off removes it. A remembered search is offered; switching
/// history off clears what was kept.
fn search_tables(
    app: &mut App,
    admin: &Client,
    shell: &Follower,
    desk: &mut Desktop,
    apps: &[ultar_desktop::model::apps::AppInfo],
    tr: &ultar_i18n::Translator,
) -> Result<(), String> {
    let follow = |desk: &mut Desktop, key: &str, value: SettingValue| {
        shell.told(key, &value)?;
        apply_setting(desk, key, &value);
        Ok::<(), String>(())
    };
    let finds_heart = |desk: &Desktop| {
        search_in("heart", apps, tr, desk.overview.sources)
            .iter()
            .any(|hit| matches!(hit, Hit::Character(_)))
    };
    if !finds_heart(desk) {
        return Err("the shell's search does not offer characters".into());
    }
    let off = change(
        (&mut *app, admin),
        ("search", "search.source.characters"),
        None,
    )?;
    follow(desk, "search.source.characters", off)?;
    if finds_heart(desk) || desk.overview.sources.characters {
        return Err("the shell still searches characters".into());
    }
    desk.overview
        .history
        .record(Record::Search("weather".into()));
    let finds_weather = |desk: &Desktop| {
        search_in_with_history(
            "weather",
            apps,
            tr,
            desk.overview.sources,
            &desk.overview.history,
        )
        .iter()
        .any(|hit| matches!(hit, Hit::History(_)))
    };
    if !finds_weather(desk) {
        return Err("the shell's search does not offer history".into());
    }
    let off = change((&mut *app, admin), ("search", "search.history"), None)?;
    follow(desk, "search.history", off)?;
    if finds_weather(desk) || !desk.overview.history.entries.is_empty() {
        return Err("the shell still keeps history after switching it off".into());
    }
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
    search_tables(&mut app, admin, shell, &mut desk, &apps, &tr)?;
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
        "SETTINGS: search and accessibility pages -> shell: apps, calculator and characters not \
         searched, history cleared when off, large text, high contrast, no motion (the Settings \
         window too)"
    );
    Ok(())
}

/// Storage: the page shows the size the kernel reports for this device's storage, as the
/// volume service measured it.
fn storage(bus: &Client, run: &Run) -> Result<(), String> {
    let own = if std::path::Path::new("/data").is_dir() {
        "/data"
    } else {
        "/"
    };
    let stat = rustix::fs::statvfs(own).map_err(|e| format!("statvfs {e}"))?;
    let total = stat.f_blocks.saturating_mul(stat.f_frsize);
    let mut app = page(bus, run, "storage")?;
    let size = ultar_i18n::units::format_size(total, app.ui.translator());
    let shown = texts(&mut app);
    let this = app.ui.translator().t("app.settings.storage.internal");
    if !shown.iter().any(|t| *t == this) || !shown.iter().any(|t| t.contains(&size)) {
        return Err(format!(
            "storage page without {size} of this device: {shown:?}"
        ));
    }
    println!("SETTINGS: storage page shows the volume service's measurement ({size})");
    Ok(())
}

/// Types `text` into the row's field (select all, type, Enter) and returns the stored value.
fn edit(
    app: &mut App,
    admin: &Client,
    panel: &str,
    key: &str,
    text: &str,
) -> Result<SettingValue, String> {
    let node = control(app, panel, key)?;
    click(app, node)?;
    let ctrl_a = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let press = |app: &mut App, key, modifiers| {
        app.handle(Event::Key { key, modifiers })
            .map_err(|e| format!("{e:?}"))
    };
    press(app, Key::Char('a'), ctrl_a)?;
    for c in text.chars() {
        press(app, Key::Char(c), Modifiers::NONE)?;
    }
    press(app, Key::Enter, Modifiers::NONE)?;
    let reply = admin
        .call(&Get { key: key.into() }, WAIT)
        .map_err(|e| format!("get {e}"))?;
    reply.value.ok_or(format!("{key}: no value"))
}

/// What the update service does with a change it is told about: its settings follow, and its
/// own code maps them onto the policy under the administrator's ceiling.
struct Updater<'a> {
    told: &'a Follower,
    settings: UpdateSettings,
}

impl Updater<'_> {
    fn follow(&mut self, key: &str, value: SettingValue) -> Result<(), String> {
        self.told.told(key, &value)?;
        self.settings
            .take(key, &value)
            .then_some(())
            .ok_or(format!("the update service ignores {key}={value:?}"))
    }
}

/// Updates page: the switches and the window through the page, the administrator-only channel
/// as the administrator; the update service's settings follow and become its policy.
fn updates(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let updated = Follower::connect(run, "the update service", "updated")?;
    let mut u = Updater {
        told: &updated,
        settings: UpdateSettings::default(),
    };
    let mut app = page(bus, run, "updates")?;
    for key in [
        "updates.auto_system",
        "updates.metered",
        "updates.offline_usb",
        "updates.notify",
    ] {
        let value = change((&mut app, admin), ("updates", key), None)?;
        u.follow(key, value)?;
    }
    let from = edit(&mut app, admin, "updates", "updates.window.from", "01:00")?;
    u.follow("updates.window.from", from)?;
    let until = edit(&mut app, admin, "updates", "updates.window.until", "04:30")?;
    u.follow("updates.window.until", until)?;
    // The channel is administrator-only (the page shows it read-only); set it as one.
    let channel = SettingValue::Text("preview".into());
    admin
        .call(
            &Set {
                key: "updates.channel".into(),
                value: channel.clone(),
            },
            WAIT,
        )
        .map_err(|e| format!("set channel {e}"))?;
    u.follow("updates.channel", channel)?;

    let p = u.settings.policy(&Policy::default(), &Ceiling::default());
    if p.automatic || !p.allow_metered || p.channel != "preview" || p.window != Some((60, 270)) {
        return Err(format!("the update policy is {p:?}"));
    }
    if u.settings.offline_usb || u.settings.notify {
        return Err("the drive offer / notifications were not switched off".into());
    }
    println!(
        "SETTINGS: updates page -> update service: automatic off, window 01:00–04:30, preview \
         channel, metered allowed, drive offer and notifications off"
    );
    Ok(())
}

/// Everything above.
pub fn wired(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let displayd = Follower::connect(run, "the display service", "displayd")?;
    let desktop = Follower::connect(run, "the shell", "desktop")?;
    input(bus, admin, run, &displayd)?;
    shell(bus, admin, run, &desktop)?;
    storage(bus, run)?;
    updates(bus, admin, run)
}
