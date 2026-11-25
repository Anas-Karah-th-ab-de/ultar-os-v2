//! Users and Emergency: the lock screen, the account service, the session service and
//! the call service are told (each with its own identity on the bus) and their own code
//! turns the values into the effect – the user list, the parental window, automatic
//! sign-in, the proposed kind, the emergency panel with its call and the SOS light.

use super::wired::{change, Follower};
use super::{click, open, setup::Run, texts};
use std::time::Duration;
use ultar_bus::messages::auth::{Secret, UserInfo};
use ultar_bus::messages::settings::{Set, SettingValue};
use ultar_bus::Client;
use ultar_desktop::lock::{emergency, prefs::LockPrefs, view as lock_view, LockState, SignInError};
use ultar_desktop::view::{Look, Target};
use ultar_i18n::Lang;
use ultar_sessiond::parental::{Due, Parental};
use ultar_settings::action::Act;
use ultar_settings::areas::index_of;
use ultar_settings::App;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

const WAIT: Duration = Duration::from_secs(20);

/// Sets a text value the way the page's text row does (`settings.set`), and returns it.
fn set_text(admin: &Client, key: &str, value: &str) -> Result<SettingValue, String> {
    let value = SettingValue::Text(value.into());
    let call = Set {
        key: key.into(),
        value: value.clone(),
    };
    admin
        .call(&call, WAIT)
        .map_err(|e| format!("set {key}: {e}"))?;
    Ok(value)
}

fn account(name: &str, admin: bool) -> UserInfo {
    UserInfo {
        name: name.into(),
        display_name: name.into(),
        has_pin: false,
        admin,
    }
}

/// The lock screen was told, and its own settings code takes the value.
fn follow_lock(
    lock: &Follower,
    prefs: &mut LockPrefs,
    key: &str,
    value: &SettingValue,
) -> Result<(), String> {
    lock.told(key, value)?;
    prefs.on_changed(key, value);
    Ok(())
}

/// The lock screen's view of a state, and every text it shows.
fn lock_screen(s: &LockState) -> Result<(ultar_desktop::view::View, Vec<String>), String> {
    let tr = ultar_desktop::i18n::translator(Lang::En).map_err(|e| format!("{e:?}"))?;
    let look = Look::new(Lang::En, ThemeKind::Dark);
    let mut v = lock_view::build(s, (0, None), look, Size::new(1280.0, 800.0), &tr)
        .map_err(|e| format!("lock view: {e:?}"))?
        .ok_or("lock view: hidden")?;
    v.ui.layout();
    let tree = v.ui.a11y_tree();
    let shown = tree
        .iter()
        .flat_map(|n| [n.name.clone(), n.value.clone(), n.description.clone()])
        .flatten()
        .map(|l| l.resolve(&tr).into_owned())
        .collect();
    Ok((v, shown))
}

fn users_page(bus: &Client, run: &Run) -> Result<App, String> {
    let area = index_of("users").ok_or("area users")?;
    open(bus, run, Lang::En, Size::new(1280.0, 800.0), area)
}

/// users.list_on_login through its page: off, the sign-in shows no account cards.
fn user_list(
    bus: &Client,
    admin: &Client,
    run: &Run,
    lock: &Follower,
    prefs: &mut LockPrefs,
) -> Result<(), String> {
    let mut app = users_page(bus, run)?;
    let off = change((&mut app, admin), ("users", "users.list_on_login"), None)?;
    follow_lock(lock, prefs, "users.list_on_login", &off)?;
    let mut s = LockState {
        user: Some(account("anas", true)),
        users: vec![account("anas", true), account("leila", false)],
        prefs: prefs.clone(),
        ..LockState::default()
    };
    s.locked();
    s.show_sign_in();
    let (v, _) = lock_screen(&s)?;
    if v.node(&Target::User(0)).is_some() {
        return Err("the sign-in lists users although users.list_on_login is off".into());
    }
    let on = change((&mut app, admin), ("users", "users.list_on_login"), None)?;
    follow_lock(lock, prefs, "users.list_on_login", &on)?;
    s.prefs = prefs.clone();
    let (v, _) = lock_screen(&s)?;
    if v.node(&Target::User(1)).is_none() {
        return Err("the sign-in does not list the users again".into());
    }
    Ok(())
}

/// users.parental: the lock screen refuses a standard user outside the window; the
/// session service is told too and its own window code agrees.
fn parental_window(
    bus: &Client,
    admin: &Client,
    run: &Run,
    lock: &Follower,
    sessiond: &Follower,
    prefs: &mut LockPrefs,
) -> Result<(), String> {
    let mut app = users_page(bus, run)?;
    let on = change((&mut app, admin), ("users", "users.parental"), None)?;
    follow_lock(lock, prefs, "users.parental", &on)?;
    sessiond.told("users.parental", &on)?;
    let mut parental = Parental::default();
    parental.on_changed("users.parental", &on);
    for (key, text) in [
        ("users.parental.from", "22:00"),
        ("users.parental.until", "06:00"),
    ] {
        let value = set_text(admin, key, text)?;
        follow_lock(lock, prefs, key, &value)?;
        sessiond.told(key, &value)?;
        parental.on_changed(key, &value);
    }
    // A window over midnight: midday is outside, 23:00 inside.
    if parental.due(12 * 60) != Due::End || parental.due(23 * 60) != Due::Nothing {
        return Err(format!("the session service's window is {parental:?}"));
    }
    let mut s = LockState {
        user: Some(account("leila", false)),
        prefs: prefs.clone(),
        ..LockState::default()
    };
    s.locked();
    s.show_sign_in();
    if s.submit(Secret::new("pw".into()), 0, 12 * 60).is_some()
        || s.error != Some(SignInError::OutsideHours)
    {
        return Err("the lock screen let a standard user in outside the window".into());
    }
    s.user = Some(account("anas", true));
    if s.submit(Secret::new("pw".into()), 0, 12 * 60).is_none() {
        return Err("the lock screen restricted an administrator".into());
    }
    let off = change((&mut app, admin), ("users", "users.parental"), None)?;
    follow_lock(lock, prefs, "users.parental", &off)?;
    sessiond.told("users.parental", &off)?;
    // The schema defaults again for whoever runs after this.
    for (key, text) in [
        ("users.parental.from", "07:00"),
        ("users.parental.until", "21:00"),
    ] {
        let value = set_text(admin, key, text)?;
        follow_lock(lock, prefs, key, &value)?;
        sessiond.told(key, &value)?;
    }
    Ok(())
}

/// users.auto_login: the account service's own parser marks the named user, the lock
/// screen carries the name for the fresh start; an empty name means nobody.
fn auto_login(
    admin: &Client,
    lock: &Follower,
    accountd: &Follower,
    prefs: &mut LockPrefs,
) -> Result<(), String> {
    let value = set_text(admin, "users.auto_login", "anas")?;
    accountd.told("users.auto_login", &value)?;
    follow_lock(lock, prefs, "users.auto_login", &value)?;
    if ultar_accountd::settings::auto_login_user(&value).as_deref() != Some("anas")
        || prefs.auto_login != "anas"
    {
        return Err("users.auto_login did not reach the account service and the lock".into());
    }
    let value = set_text(admin, "users.auto_login", "")?;
    accountd.told("users.auto_login", &value)?;
    follow_lock(lock, prefs, "users.auto_login", &value)?;
    if ultar_accountd::settings::auto_login_user(&value).is_some() {
        return Err("an empty users.auto_login still names a user".into());
    }
    Ok(())
}

/// users.new_kind: the Users page proposes the configured kind in its add dialog.
fn new_kind(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    set_text(admin, "users.new_kind", "administrator")?;
    let mut app = users_page(bus, run)?;
    if app.state.new_user_kind() != ultar_bus::messages::account_local::UserKind::Administrator {
        return Err("the Users page does not propose users.new_kind".into());
    }
    let add = app.view.node(Act::UserAdd).ok_or("no add-user button")?;
    click(&mut app, add)?;
    let shown = texts(&mut app);
    if !shown.iter().any(|t| t.contains("administrator rights")) {
        return Err(format!("the add-user dialog hides the kind: {shown:?}"));
    }
    set_text(admin, "users.new_kind", "standard")?;
    Ok(())
}

/// emergency.on_lock_screen through its page, the details as the text rows set them:
/// the cover's button is usable, the panel shows exactly what was stored, the call
/// service validates the number, the SOS light flashes.
fn emergency_panel(
    bus: &Client,
    admin: &Client,
    run: &Run,
    lock: &Follower,
    prefs: &mut LockPrefs,
) -> Result<(), String> {
    let area = index_of("emergency").ok_or("area emergency")?;
    let mut app = open(bus, run, Lang::En, Size::new(1280.0, 800.0), area)?;
    let on = change(
        (&mut app, admin),
        ("emergency", "emergency.on_lock_screen"),
        None,
    )?;
    follow_lock(lock, prefs, "emergency.on_lock_screen", &on)?;
    for (key, text) in [
        ("emergency.contact_name", "Layla"),
        ("emergency.contact_phone", "+963112223333"),
        ("emergency.medical", "Blood type A+"),
        ("emergency.number", "999"),
    ] {
        let value = set_text(admin, key, text)?;
        follow_lock(lock, prefs, key, &value)?;
    }
    let mut s = LockState {
        prefs: prefs.clone(),
        ..LockState::default()
    };
    s.locked();
    let (v, _) = lock_screen(&s)?;
    let sos = v.node(&Target::Emergency).ok_or("no emergency button")?;
    if v.ui.is_disabled(sos) {
        return Err("the emergency button stays disabled although switched on".into());
    }
    s.emergency.flip(prefs.emergency_on);
    let (v, shown) = lock_screen(&s)?;
    for want in ["Layla", "+963112223333", "Blood type A+", "999"] {
        if !shown.iter().any(|t| t.contains(want)) {
            return Err(format!("the emergency panel hides {want}: {shown:?}"));
        }
    }
    if v.node(&Target::EmergencyCall).is_none() {
        return Err("the emergency panel has no call button".into());
    }
    if ultar_call::emergency::of(&SettingValue::Text("999".into())) != "999"
        || ultar_call::emergency::of(&SettingValue::Text("bad".into())) != "112"
    {
        return Err("the call service does not validate emergency.number".into());
    }
    if !emergency::sos_light_on(0) || emergency::sos_light_on(200) {
        return Err("the SOS light is not three short, three long, three short".into());
    }
    emergency_defaults(admin, lock, prefs, &mut app)
}

/// Back to the defaults for whoever runs after this.
fn emergency_defaults(
    admin: &Client,
    lock: &Follower,
    prefs: &mut LockPrefs,
    app: &mut App,
) -> Result<(), String> {
    let off = change(
        (app, admin),
        ("emergency", "emergency.on_lock_screen"),
        None,
    )?;
    follow_lock(lock, prefs, "emergency.on_lock_screen", &off)?;
    for key in [
        "emergency.contact_name",
        "emergency.contact_phone",
        "emergency.medical",
    ] {
        let value = set_text(admin, key, "")?;
        follow_lock(lock, prefs, key, &value)?;
    }
    let value = set_text(admin, "emergency.number", "112")?;
    follow_lock(lock, prefs, "emergency.number", &value)?;
    Ok(())
}

/// Everything above, with the followers of the four services.
pub fn users_emergency(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let lock = Follower::connect(run, "the lock screen", "lock")?;
    let accountd = Follower::connect(run, "the account service", "accountd")?;
    let sessiond = Follower::connect(run, "the session service", "sessiond")?;
    let mut prefs = LockPrefs::default();
    user_list(bus, admin, run, &lock, &mut prefs)?;
    parental_window(bus, admin, run, &lock, &sessiond, &mut prefs)?;
    auto_login(admin, &lock, &accountd, &mut prefs)?;
    new_kind(bus, admin, run)?;
    emergency_panel(bus, admin, run, &lock, &mut prefs)?;
    println!(
        "SETTINGS: users and emergency pages -> lock screen, account service, session \
         service, call service: user list, parental window, automatic sign-in, new kind, \
         emergency panel with its call and SOS light"
    );
    Ok(())
}
