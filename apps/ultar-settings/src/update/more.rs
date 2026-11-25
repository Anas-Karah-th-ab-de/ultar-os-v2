//! Clicks of the Users, Apps, Storage and Updates pages: local users through the account
//! service (it asks for the administrator itself), taking an app's permission back through
//! the launcher, the mounted drives, and the updater's progress.

use super::{ask, close, dialog_error, fields, Ctl, Outcome};
use crate::action::Act;
use crate::service::{PortResult, Ports};
use crate::state::{DialogPurpose, State};
use ultar_bus::messages::account_local::LocalUser;
use ultar_bus::Text;
use ultar_ui::event::ActionKind;

/// Shortest password of a local user.
pub const MIN_PASSWORD: usize = 6;

/// Reads what the page of settingsd panel `panel` shows besides its rows.
pub fn load(state: &mut State, ports: &mut Ports, panel: &str) {
    let more = &mut state.more;
    match panel {
        "users" => match ports.system.users() {
            Ok(users) => (more.users, more.users_error) = (users, None),
            Err(e) => more.users_error = Some(e.text()),
        },
        "apps" => {
            match ports.system.apps() {
                Ok(apps) => (more.apps, more.apps_error) = (apps, None),
                Err(e) => more.apps_error = Some(e.text()),
            }
            // the default-app rows offer the installed apps that declare each kind
            more.candidates = ultar_bus::messages::launcher::Kind::ALL
                .into_iter()
                .map(|k| (k.key(), ports.system.candidates(k).unwrap_or_default()))
                .collect();
        }
        "storage" => match ports.system.drives() {
            Ok(drives) => (more.drives, more.drives_error) = (drives, None),
            Err(e) => more.drives_error = Some(e.text()),
        },
        "updates" => state.drivers.firmware = ports.update.progress(),
        _ => {}
    }
}

/// `true` for the actions of these pages.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::UserAdd | Act::UserRemove(_) | Act::UserPassword(_) | Act::AppRevoke(_, _)
    )
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, kind: ActionKind) -> Outcome {
    match (act, kind) {
        (Act::UserAdd, ActionKind::Activated) => ask(c.state, DialogPurpose::UserAdd),
        (Act::UserRemove(n), ActionKind::Activated) => ask(c.state, DialogPurpose::UserRemove(n)),
        (Act::UserPassword(n), ActionKind::Activated) => {
            ask(c.state, DialogPurpose::UserPassword(n))
        }
        (Act::AppRevoke(app, permission), ActionKind::Toggled(false)) => revoke(c, app, permission),
        _ => Outcome::default(),
    }
}

fn revoke(c: &mut Ctl<'_>, app: usize, permission: usize) -> Outcome {
    let Some(entry) = c.state.more.apps.get(app) else {
        return Outcome::default();
    };
    let Some((name, _)) = entry.permissions.get(permission) else {
        return Outcome::default();
    };
    let (id, name) = (entry.id.clone(), name.clone());
    let result = c.ports.system.app_revoke(&id, &name);
    load(c.state, c.ports, "apps");
    match result {
        Ok(_) => Outcome::rebuild(),
        Err(e) => Outcome {
            rebuild: true,
            ..Outcome::error(&e)
        },
    }
}

fn user_name(state: &State, n: usize) -> Option<String> {
    state.more.users.get(n).map(|u| u.name.clone())
}

/// The two password fields `first` and `first + 1` when they are equal and long enough.
fn new_password(f: &[String], first: usize) -> Result<String, Text> {
    let [new, again] = [first, first + 1].map(|i| f.get(i).cloned().unwrap_or_default());
    if new != again {
        return Err(Text::key("app.settings.dialog.pw.differ"));
    }
    if new.chars().count() < MIN_PASSWORD {
        return Err(Text::key("app.settings.users.password.short"));
    }
    Ok(new)
}

/// A dialog of the Users page was confirmed.
pub fn dialog_ok(c: &mut Ctl<'_>, purpose: DialogPurpose) -> Outcome {
    let f = fields(c);
    let result: PortResult<Vec<LocalUser>> = match purpose {
        DialogPurpose::UserAdd => {
            let name = f.first().map(|n| n.trim().to_owned()).unwrap_or_default();
            if name.is_empty() {
                return dialog_error(c.state, Text::key("app.settings.account.fill_in"));
            }
            match new_password(&f, 1) {
                // users.new_kind: the kind the page proposes (standard/administrator).
                Ok(password) => {
                    let kind = c.state.new_user_kind();
                    c.ports.system.user_create(&name, &password, kind)
                }
                Err(text) => return dialog_error(c.state, text),
            }
        }
        DialogPurpose::UserPassword(n) => {
            let Some(name) = user_name(c.state, n) else {
                return close(c.state);
            };
            match new_password(&f, 0) {
                Ok(password) => c.ports.system.user_password(&name, &password),
                Err(text) => return dialog_error(c.state, text),
            }
        }
        DialogPurpose::UserRemove(n) => match user_name(c.state, n) {
            Some(name) => c.ports.system.user_remove(&name),
            None => return close(c.state),
        },
        _ => return close(c.state),
    };
    match result {
        Ok(users) => {
            c.state.more.users = users;
            c.state.more.users_error = None;
            c.state.dialog = None;
            Outcome {
                dialog: true,
                rebuild: true,
                ..Outcome::default()
            }
        }
        Err(e) => dialog_error(c.state, e.text()),
    }
}
