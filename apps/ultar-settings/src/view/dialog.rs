//! Dialogs of the app: driver switch, update from a drive, change password, delete account,
//! remove device, QR code. The "keep these display settings?" question is not here: the
//! system dialog service asks it (it follows `settings.pending`).

use super::{k, muted, View};
use crate::action::Act;
use crate::state::{DialogPurpose, State};
use ultar_ui::layout::Flex;
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Dialog, InfoBar, Label, Text, TextDirection, TextField, Tone,
};

/// Longest text typed into a dialog field.
pub const MAX_FIELD: usize = 512;

struct Spec {
    title: &'static str,
    message: Label,
    fields: Vec<(&'static str, bool)>,
    ok: &'static str,
    danger: bool,
}

fn plain(
    title: &'static str,
    message: Label,
    ok: &'static str,
    fields: &[(&'static str, bool)],
    danger: bool,
) -> Spec {
    Spec {
        title,
        message,
        fields: fields.to_vec(),
        ok,
        danger,
    }
}

fn driver_name(state: &State, row: usize, alt: usize) -> String {
    let d = state.drivers.devices.get(row);
    let offered = d.map(crate::update::offered_drivers).unwrap_or_default();
    offered
        .get(alt)
        .map_or_else(String::new, |a| format!("{} {}", a.id, a.version))
}

fn device_name(state: &State, n: usize) -> String {
    let devices = state.account.state.as_ref().map(|s| s.devices.as_slice());
    devices
        .and_then(|d| d.get(n))
        .map_or_else(String::new, |d| d.name.clone())
}

const PW_FIELDS: [(&str, bool); 3] = [
    ("app.settings.dialog.pw.old", true),
    ("app.settings.dialog.pw.new", true),
    ("app.settings.dialog.pw.again", true),
];

/// "Forget …?": a question without fields whose answer cannot be taken back.
fn forget(ok: &'static str, title: &'static str, text: &'static str) -> Spec {
    plain(title, k(text), ok, &[], true)
}

fn spec(state: &State, purpose: DialogPurpose) -> Spec {
    let d = "app.settings.dialog.";
    match purpose {
        DialogPurpose::DriverSwitch { row, alt } => {
            let name = driver_name(state, row, alt);
            let text = Label::key_with(format!("{d}switch.text"), &[("driver", &name)]);
            plain(
                "app.settings.dialog.switch",
                text,
                "app.settings.dialog.switch.ok",
                &[],
                false,
            )
        }
        DialogPurpose::RemoveDevice(n) => {
            let name = device_name(state, n);
            let text = Label::key_with(format!("{d}remove.text"), &[("name", &name)]);
            let ok = "app.settings.account.device.remove";
            plain("app.settings.dialog.remove", text, ok, &[], true)
        }
        DialogPurpose::UserRemove(n) => {
            let name = state.more.users.get(n).map_or("", |u| u.name.as_str());
            let text = Label::key_with("app.settings.users.remove.text", &[("name", name)]);
            let ok = "app.settings.users.remove";
            plain("app.settings.users.remove.title", text, ok, &[], true)
        }
        // users.new_kind: the dialog says which kind the new user will be.
        DialogPurpose::UserAdd => user_add(state),
        DialogPurpose::MeshPairShow | DialogPurpose::MeshPairJoin | DialogPurpose::MeshUnpair => {
            mesh(state, purpose)
        }
        // the key is typed hidden and never shown again
        DialogPurpose::CloudKey => plain(
            "app.settings.cloud.key.title",
            k("app.settings.cloud.key.text"),
            "app.settings.cloud.key.save",
            &[("app.settings.cloud.key.field", true)],
            false,
        ),
        DialogPurpose::CloudAddress => plain(
            "app.settings.cloud.address.title",
            k("app.settings.cloud.address.text"),
            "app.settings.cloud.key.save",
            &[("app.settings.cloud.address", false)],
            false,
        ),
        DialogPurpose::CloudClear => forget(
            "app.settings.cloud.clear",
            "app.settings.cloud.clear.title",
            "app.settings.cloud.clear.text",
        ),
        DialogPurpose::HelperForget => forget(
            "app.settings.helper.forget",
            "app.settings.helper.forget.title",
            "app.settings.helper.forget.text",
        ),
        DialogPurpose::ForeignRemove => foreign_remove(state),
        other => fixed(other),
    }
}

/// "Remove this app from another system?": says whether its data stays.
fn foreign_remove(state: &State) -> Spec {
    let f = &state.status.foreign;
    let name = f.app().map_or("", |a| a.name.as_str());
    let text = if f.keep_data {
        "app.settings.foreign.remove.text.keep"
    } else {
        "app.settings.foreign.remove.text"
    };
    plain(
        "app.settings.foreign.remove.title",
        Label::key_with(text, &[("name", name)]),
        "app.settings.foreign.remove",
        &[],
        true,
    )
}

/// The three dialogs of Settings › Mesh (boards *MeshPairShow*, *MeshPairJoin*; unpair).
fn mesh(state: &State, purpose: DialogPurpose) -> Spec {
    let m = &state.status.mesh;
    let near = m.near();
    let joined = near.iter().find(|p| Some(&p.id) == m.join.as_ref());
    let name = match purpose {
        DialogPurpose::MeshPairJoin => joined.map_or("", |p| p.name.as_str()),
        _ => m.open().map_or("", |p| p.name.as_str()),
    };
    let text = |key: &str| Label::key_with(key, &[("name", name)]);
    match purpose {
        DialogPurpose::MeshPairShow => plain(
            "app.settings.mesh.pair.show",
            k("app.settings.mesh.pair.show.text"),
            "app.settings.cancel",
            &[],
            false,
        ),
        DialogPurpose::MeshPairJoin => plain(
            "app.settings.mesh.pair.join.title",
            text("app.settings.mesh.pair.join.text"),
            "app.settings.mesh.pair.join",
            &[("app.settings.mesh.pair.code", false)],
            false,
        ),
        _ => plain(
            "app.settings.mesh.unpair.title",
            text("app.settings.mesh.unpair.text"),
            "app.settings.mesh.unpair",
            &[],
            true,
        ),
    }
}

/// What a dialog shows large: the account number, or the pairing code in two groups.
fn large(state: &State, purpose: DialogPurpose) -> Option<String> {
    match purpose {
        DialogPurpose::Qr => state.account.state.as_ref().map(|a| a.number.clone()),
        DialogPurpose::MeshPairShow => {
            let code = &state.status.mesh.code.as_ref()?.code;
            let (a, b) = code.split_at(code.len().min(3));
            Some(format!("{a} {b}"))
        }
        _ => None,
    }
}

const USER_FIELDS: [(&str, bool); 3] = [
    ("app.settings.users.name", false),
    ("app.settings.dialog.pw.new", true),
    ("app.settings.dialog.pw.again", true),
];

/// The add-user dialog: its text says the kind `users.new_kind` proposes.
fn user_add(state: &State) -> Spec {
    let text = match state.new_user_kind() {
        ultar_bus::messages::account_local::UserKind::Administrator => {
            "app.settings.users.add.text.admin"
        }
        _ => "app.settings.users.add.text",
    };
    plain(
        "app.settings.users.add",
        k(text),
        "app.settings.users.add",
        &USER_FIELDS,
        false,
    )
}

fn fixed(purpose: DialogPurpose) -> Spec {
    let (title, text, ok, fields, danger): (_, _, _, &[(&str, bool)], _) = match purpose {
        DialogPurpose::FirmwareUsb => (
            "app.settings.dialog.usb",
            "app.settings.dialog.usb.text",
            "app.settings.dialog.usb.ok",
            &[("app.settings.dialog.usb.path", false)],
            false,
        ),
        DialogPurpose::ChangePassword => (
            "app.settings.account.change_password",
            "app.settings.dialog.pw.text",
            "app.settings.dialog.pw.ok",
            &PW_FIELDS,
            false,
        ),
        DialogPurpose::UserPassword(_) => (
            "app.settings.users.password",
            "app.settings.users.password.text",
            "app.settings.dialog.pw.ok",
            &USER_FIELDS[1..],
            false,
        ),
        DialogPurpose::DeleteAccount => (
            "app.settings.dialog.delete",
            "app.settings.dialog.delete.text",
            "app.settings.account.delete",
            &[("app.settings.account.password", true)],
            true,
        ),
        _ => (
            "app.settings.account.qr",
            "app.settings.dialog.qr.text",
            "app.settings.close",
            &[],
            false,
        ),
    };
    plain(title, k(text), ok, fields, danger)
}

/// Builds and opens the dialog of `state.dialog` (closes any other first).
pub fn open(ui: &mut Ui, state: &State, view: &mut View) -> Result<(), UiError> {
    if let Some(old) = view.dialog.take() {
        ui.close_dialog(old)?;
        ui.remove(old)?;
    }
    view.page
        .retain(|_, a| !matches!(a, Act::DialogOk | Act::DialogCancel | Act::DialogField(_)));
    view.dialog_fields.clear();
    let (Some(dialog), Some(parent)) = (state.dialog.as_ref(), view.content) else {
        return Ok(());
    };
    let s = spec(state, dialog.purpose);
    let mut widget = Dialog::new(k(s.title)).flex(Flex::column().gap(Space::S16));
    if s.danger {
        widget = widget.alert();
    }
    let id = ui.add(parent, widget)?;
    view.dialog = Some(id);
    muted(ui, id, s.message.clone())?;
    if let Some(text) = large(state, dialog.purpose) {
        let big = Text::new(Label::data(&text)).spec(TextSpec::title(TypeSize::S32));
        ui.add(id, big)?;
    }
    for (i, (key, secret)) in s.fields.iter().enumerate() {
        let field = if *secret {
            TextField::password(k(key))
        } else {
            TextField::new(k(key)).direction(TextDirection::Ltr)
        };
        let f = ui.add(id, field.labelled().max_len(MAX_FIELD))?;
        view.page.add(f, Act::DialogField(i));
        view.dialog_fields.push(f);
    }
    if let Some(error) = &dialog.error {
        let label = crate::i18n::label(error, ui.translator());
        ui.add(id, InfoBar::new(Tone::Danger, label))?;
    }
    let one_button = matches!(
        dialog.purpose,
        DialogPurpose::Qr | DialogPurpose::MeshPairShow
    );
    buttons(ui, view, id, one_button, &s)?;
    ui.open_dialog(id)
}

/// Cancel (not for the QR code, which only closes) and the confirming button.
fn buttons(
    ui: &mut Ui,
    view: &mut View,
    id: NodeId,
    close_only: bool,
    s: &Spec,
) -> Result<(), UiError> {
    let end = Flex::row()
        .gap(Space::S8)
        .main(ultar_ui::layout::MainAlign::End);
    let buttons = ui.add(id, end)?;
    let mut cancel = None;
    if !close_only {
        let c = ui.add(buttons, Button::new(k("app.settings.cancel")))?;
        view.page.add(c, Act::DialogCancel);
        cancel = Some(c);
    }
    let variant = if s.danger {
        ButtonVariant::Danger
    } else {
        ButtonVariant::Primary
    };
    let ok = ui.add(buttons, Button::new(k(s.ok)).variant(variant))?;
    view.page.add(ok, Act::DialogOk);
    ui.set_default_action(id, Some(ok))?;
    ui.set_cancel_action(id, cancel.or(Some(ok)))
}
