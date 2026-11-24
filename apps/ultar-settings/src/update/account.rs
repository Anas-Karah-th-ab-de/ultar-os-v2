//! Clicks of the Ultar account page (`account.*`, DESKTOP-SPEC §12.1).

use super::{ask, close, dialog_error, fields, Ctl, Outcome};
use crate::action::Act;
use crate::areas::{index_of_page, PageKind};
use crate::service::{AccountRequest, PortError, Ports};
use crate::state::{DialogPurpose, State, SubPage};
use crate::view::account_strength;
use ultar_bus::messages::account_service::{AccountState, MAX_PASSWORD, MAX_SERVER, MAX_USER};
use ultar_bus::Text;
use ultar_ui::event::ActionKind;
use ultar_ui::ui::NodeId;
use ultar_ui::widgets::{Label, Tone};

/// Shortest password accepted for a new account or a new password.
pub const MIN_PASSWORD: usize = 8;

/// Reads the account service.
pub fn load(state: &mut State, ports: &mut Ports) {
    match ports.account.state() {
        Ok(s) => {
            if state.account.server.is_empty() {
                state.account.server.clone_from(&s.server);
            }
            state.account.state = Some(s);
            state.account.error = None;
        }
        Err(e) => {
            state.account.state = None;
            state.account.error = Some(e.text());
        }
    }
}

/// `true` for the actions of this page.
pub fn owns(act: Act) -> bool {
    matches!(
        act,
        Act::AccountBack
            | Act::AccountUser
            | Act::AccountPassword
            | Act::AccountShow
            | Act::AccountSignIn
            | Act::AccountCreate
            | Act::AccountServer
            | Act::AccountCopy
            | Act::AccountQr
            | Act::AccountServiceSwitch(_)
            | Act::AccountChangePassword
            | Act::AccountSignOut
            | Act::AccountDelete
            | Act::AccountDeviceSignOut(_)
            | Act::AccountDeviceRemove(_)
    )
}

fn apply(state: &mut State, result: Result<AccountState, PortError>) -> Outcome {
    match result {
        Ok(s) => {
            state.account.state = Some(s);
            state.account.error = None;
            state.account.copied = false;
            Outcome::rebuild()
        }
        Err(e) => Outcome::error(&e),
    }
}

fn text(c: &Ctl<'_>, node: NodeId) -> String {
    c.ui.text(node).map(str::to_owned).unwrap_or_default()
}

/// Handles a click.
pub fn handle(c: &mut Ctl<'_>, act: Act, node: NodeId, kind: ActionKind) -> Outcome {
    use ActionKind::{Activated, Submitted, TextChanged, Toggled};
    match (act, kind) {
        (Act::AccountBack, Activated) => back(c.state),
        (Act::AccountUser, TextChanged) => {
            c.state.account.user = text(c, node);
            Outcome::default()
        }
        (Act::AccountPassword, TextChanged) => strength(c, node),
        (Act::AccountServer, TextChanged) => {
            c.state.account.server = text(c, node);
            Outcome::default()
        }
        (Act::AccountServer, Submitted) => server(c),
        (Act::AccountSignIn | Act::AccountPassword, Activated | Submitted) => sign_in(c),
        (Act::AccountCreate, Activated) => {
            c.state.sub = SubPage::CreateAccount;
            Outcome::rebuild()
        }
        (Act::AccountCopy, Activated) => copy(c),
        (Act::AccountQr, Activated) => ask(c.state, DialogPurpose::Qr),
        (Act::AccountServiceSwitch(s), Toggled(on)) => {
            let r = c.ports.account.request(AccountRequest::SetService(s, on));
            apply(c.state, r)
        }
        (Act::AccountChangePassword, Activated) => ask(c.state, DialogPurpose::ChangePassword),
        (Act::AccountSignOut, Activated) => {
            let r = c.ports.account.request(AccountRequest::SignOut);
            apply(c.state, r)
        }
        (Act::AccountDelete, Activated) => ask(c.state, DialogPurpose::DeleteAccount),
        (Act::AccountDeviceSignOut(n), Activated) => match device_id(c.state, n) {
            Some(id) => {
                let r = c.ports.account.request(AccountRequest::DeviceSignOut(id));
                apply(c.state, r)
            }
            None => Outcome::default(),
        },
        (Act::AccountDeviceRemove(n), Activated) => ask(c.state, DialogPurpose::RemoveDevice(n)),
        _ => Outcome::default(),
    }
}

/// Back: compact – to the list of areas; wide – to the area the user came from.
fn back(state: &mut State) -> Outcome {
    state.sub = SubPage::None;
    if state.compact {
        state.compact_list = true;
        return Outcome {
            rebuild_all: true,
            ..Outcome::default()
        };
    }
    let fallback = index_of_page(PageKind::Display).unwrap_or(0);
    state.area = state.prev_area.take().unwrap_or(fallback);
    Outcome::rebuild()
}

fn device_id(state: &State, n: usize) -> Option<String> {
    state
        .account
        .state
        .as_ref()?
        .devices
        .get(n)
        .map(|d| d.id.clone())
}

fn strength(c: &mut Ctl<'_>, node: NodeId) -> Outcome {
    let level = account_strength(&text(c, node));
    let live: Vec<NodeId> = c
        .view
        .live
        .iter()
        .filter(|(a, _)| *a == Act::AccountPassword)
        .map(|(_, n)| *n)
        .collect();
    if let [label, bar, ..] = live[..] {
        let _ = c.ui.set_label(
            label,
            Label::key(format!("app.settings.account.strength.{level}")),
        );
        let _ = c.ui.set_progress(bar, Some(f32::from(level) / 4.0));
    }
    Outcome::default()
}

fn password_node(c: &Ctl<'_>) -> Option<NodeId> {
    c.view.node(Act::AccountPassword)
}

fn sign_in(c: &mut Ctl<'_>) -> Outcome {
    let user = c.state.account.user.trim().to_owned();
    let password = password_node(c).map(|n| text(c, n)).unwrap_or_default();
    if user.is_empty()
        || user.len() > MAX_USER
        || password.is_empty()
        || password.len() > MAX_PASSWORD
    {
        return Outcome::toast(Tone::Warning, "app.settings.account.fill_in");
    }
    let r = c
        .ports
        .account
        .request(AccountRequest::SignIn { user, password });
    if let Some(n) = password_node(c) {
        let _ = c.ui.set_text(n, "");
    }
    match r {
        Ok(s) => apply(c.state, Ok(s)),
        Err(PortError::Refused(reason)) => {
            load(c.state, c.ports);
            if let Some(s) = c.state.account.state.as_mut() {
                s.error.get_or_insert(reason);
            }
            Outcome::rebuild()
        }
        Err(e) => Outcome::error(&e),
    }
}

fn server(c: &mut Ctl<'_>) -> Outcome {
    let server = c.state.account.server.trim().to_owned();
    let valid = !server.is_empty()
        && server.len() <= MAX_SERVER
        && server
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':'));
    if !valid {
        return Outcome::toast(Tone::Warning, "app.settings.account.server.invalid");
    }
    let r = c.ports.account.request(AccountRequest::SetServer(server));
    let out = apply(c.state, r);
    if out.toast.is_none() {
        return Outcome {
            toast: Some((Tone::Success, Text::key("app.settings.account.server.ok"))),
            ..out
        };
    }
    out
}

fn copy(c: &mut Ctl<'_>) -> Outcome {
    let number = c
        .state
        .account
        .state
        .as_ref()
        .map(|s| s.number.clone())
        .unwrap_or_default();
    if number.is_empty() {
        return Outcome::default();
    }
    match c.ports.clipboard.copy(&number) {
        Ok(()) => {
            c.state.account.copied = true;
            if let Some(n) = c.view.node(Act::AccountCopy) {
                let _ = c.ui.set_label(n, Label::key("app.settings.account.copied"));
            }
            Outcome::toast(Tone::Success, "app.settings.account.copied")
        }
        Err(e) => Outcome::error(&e),
    }
}

/// OK of the account dialogs.
pub fn dialog_ok(c: &mut Ctl<'_>, purpose: DialogPurpose) -> Outcome {
    let f = fields(c);
    let request = match purpose {
        DialogPurpose::ChangePassword => {
            let [old, new, again] = [0, 1, 2].map(|i| f.get(i).cloned().unwrap_or_default());
            if new != again {
                return dialog_error(c.state, Text::key("app.settings.dialog.pw.differ"));
            }
            if new.chars().count() < MIN_PASSWORD {
                return dialog_error(c.state, Text::key("app.settings.dialog.pw.short"));
            }
            AccountRequest::ChangePassword { old, new }
        }
        DialogPurpose::DeleteAccount => {
            let password = f.first().cloned().unwrap_or_default();
            if password.is_empty() {
                return dialog_error(c.state, Text::key("app.settings.account.fill_in"));
            }
            AccountRequest::Delete(password)
        }
        DialogPurpose::RemoveDevice(n) => match device_id(c.state, n) {
            Some(id) => AccountRequest::DeviceRemove(id),
            None => return close(c.state),
        },
        _ => return close(c.state),
    };
    match c.ports.account.request(request) {
        Ok(s) => {
            c.state.account.state = Some(s);
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

/// OK / Cancel of the page "Create account".
pub fn create_page(c: &mut Ctl<'_>, act: Act) -> Outcome {
    if act == Act::DialogCancel {
        c.state.sub = SubPage::None;
        c.state.dialog = None;
        return Outcome::rebuild();
    }
    if act != Act::DialogOk {
        return Outcome::default();
    }
    let f = fields(c);
    let [user, password, again, email] =
        [0, 1, 2, 3].map(|i| f.get(i).cloned().unwrap_or_default());
    let error = if user.trim().is_empty() || user.len() > MAX_USER {
        Some("app.settings.account.fill_in")
    } else if password != again {
        Some("app.settings.dialog.pw.differ")
    } else if password.chars().count() < MIN_PASSWORD {
        Some("app.settings.dialog.pw.short")
    } else {
        None
    };
    if let Some(key) = error {
        c.state.dialog = Some(crate::state::DialogState {
            purpose: DialogPurpose::ChangePassword,
            error: Some(Text::key(key)),
        });
        return Outcome::rebuild();
    }
    let email = Some(email.trim().to_owned()).filter(|e| !e.is_empty());
    let request = AccountRequest::Create {
        user: user.trim().to_owned(),
        password,
        email,
    };
    match c.ports.account.request(request) {
        Ok(s) => {
            c.state.account.state = Some(s);
            c.state.sub = SubPage::None;
            c.state.dialog = None;
            Outcome::rebuild()
        }
        Err(e) => {
            c.state.dialog = Some(crate::state::DialogState {
                purpose: DialogPurpose::ChangePassword,
                error: Some(e.text()),
            });
            Outcome::rebuild()
        }
    }
}
