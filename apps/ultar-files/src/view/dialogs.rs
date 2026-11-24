//! Dialogs over the window: rename, confirm (empty trash, delete for good), properties,
//! name conflicts, a busy drive and failures (permission errors said plainly).

use super::Built;
use crate::action::Target;
use crate::fs::{Fs, Kind, Loc};
use crate::model::{file_kind, Dialog as D, State};
use ultar_ui::layout::{Flex, MainAlign};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Dialog, Label, Row, Text, TextField};

/// Adds and opens the dialog of `s`, if any.
pub fn build(ui: &mut Ui, b: &mut Built, s: &State, fs: &Fs) -> Result<(), UiError> {
    let Some(d) = &s.dialog else { return Ok(()) };
    let root = ui.root();
    let (title, alert) = match d {
        D::Rename(loc) => (
            Label::key_with(
                "files.rename.title",
                &[("name", loc.name().unwrap_or_default())],
            ),
            false,
        ),
        D::ConfirmEmpty => (Label::key("files.empty_trash.title"), true),
        D::ConfirmPurge(ids) => (
            Label::key_count("files.purge.title", "count", ids.len() as i64),
            true,
        ),
        D::Properties(loc) => (
            Label::key_with("files.properties.title", &[("name", &display_name(loc))]),
            false,
        ),
        D::Conflict(_, to) => (
            Label::key_with(
                "files.conflict.title",
                &[("name", to.name().unwrap_or_default())],
            ),
            true,
        ),
        D::DriveBusy(_) => (Label::key("files.busy.title"), true),
        D::Failed(e, n) => (Label::key_count(e.key(), "count", *n as i64), true),
    };
    let mut dialog = Dialog::new(title);
    if alert {
        dialog = dialog.alert();
    }
    let id = ui.add(root, dialog)?;
    b.dialog = Some(id);
    body(ui, b, id, s, d, fs)?;
    let ok = buttons(ui, b, id, d)?;
    ui.set_default_action(id, ok)?;
    ui.open_dialog(id)?;
    if let Some(field) = b.rename {
        ui.focus(field)?;
    }
    Ok(())
}

fn display_name(loc: &Loc) -> String {
    loc.name()
        .map(str::to_owned)
        .unwrap_or_else(|| loc.area.name())
}

fn text(ui: &mut Ui, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
    ui.add(parent, Text::new(label).color(ColorRole::TextMuted))
}

fn body(ui: &mut Ui, b: &mut Built, id: NodeId, s: &State, d: &D, fs: &Fs) -> Result<(), UiError> {
    match d {
        D::Rename(loc) => {
            let name = loc.name().unwrap_or_default();
            let field = TextField::new(Label::key("files.rename.field"))
                .text(name)
                .labelled();
            let field = ui.add(id, field)?;
            b.rename = Some(b.mark(field, Target::RenameField));
        }
        D::ConfirmEmpty => {
            text(
                ui,
                id,
                Label::key_count("files.empty_trash.message", "count", s.items.len() as i64),
            )?;
        }
        D::ConfirmPurge(_) => {
            text(ui, id, Label::key("files.purge.message"))?;
        }
        D::Conflict(..) => {
            text(ui, id, Label::key("files.conflict.message"))?;
        }
        D::DriveBusy(_) => {
            text(ui, id, Label::key("files.busy.message"))?;
        }
        D::Failed(e, _) => {
            text(ui, id, Label::key(hint(*e)))?;
        }
        D::Properties(loc) => properties(ui, id, fs, loc, s)?,
    }
    Ok(())
}

/// What to do about an error, in plain words.
pub fn hint(e: crate::fs::FsError) -> &'static str {
    use crate::fs::FsError as E;
    match e {
        E::Permission => "files.hint.permission",
        E::NoSpace => "files.hint.no_space",
        E::Link => "files.hint.link",
        E::NoArea => "files.hint.no_area",
        E::Exists | E::BadName => "files.hint.name",
        _ => "files.hint.retry",
    }
}

/// Rows of the properties dialog, read from the file system.
fn properties(ui: &mut Ui, dialog: NodeId, fs: &Fs, loc: &Loc, s: &State) -> Result<(), UiError> {
    let Ok(e) = fs.stat(loc) else {
        return text(ui, dialog, Label::key("files.error.not_found")).map(|_| ());
    };
    let tr = ui.translator();
    let kind = tr.t(file_kind(&e).key()).into_owned();
    let size = if e.kind == Kind::File {
        ultar_i18n::units::format_size(e.size, tr)
    } else {
        String::new()
    };
    let date = super::items::date(e.modified, tr);
    let place = match loc.parent() {
        Some(p) => crate::uri::of_loc(&p),
        None => String::new(),
    };
    let mode = mode_text(e.mode);
    let mut rows = vec![
        ("files.prop.kind", kind),
        ("files.prop.size", size),
        ("files.prop.modified", date),
        ("files.prop.place", place),
        ("files.prop.mode", mode),
    ];
    if let Some(t) = s
        .items
        .iter()
        .find(|i| Some(i.entry.name.as_str()) == loc.name())
        .and_then(|i| i.trashed.as_ref())
    {
        rows.push(("files.prop.origin", crate::uri::of_loc(&t.origin)));
    }
    for (key, value) in rows.into_iter().filter(|(_, v)| !v.is_empty()) {
        ui.add(
            dialog,
            Row::new(Label::key(key)).sublabel(Label::data(&value)),
        )?;
    }
    Ok(())
}

/// `rwxr-x---` of the permission bits.
pub fn mode_text(mode: u32) -> String {
    (0..9)
        .map(|i| {
            let bit = 1 << (8 - i);
            match (mode & bit != 0, i % 3) {
                (false, _) => '-',
                (true, 0) => 'r',
                (true, 1) => 'w',
                (true, _) => 'x',
            }
        })
        .collect()
}

fn buttons(ui: &mut Ui, b: &mut Built, id: NodeId, d: &D) -> Result<Option<NodeId>, UiError> {
    let row = ui.add(id, Flex::row().gap(Space::S8).main(MainAlign::End))?;
    let (cancel, alt, ok, danger) = match d {
        D::Rename(_) => (Some("files.cancel"), None, Some("files.cmd.rename"), false),
        D::ConfirmEmpty => (
            Some("files.cancel"),
            None,
            Some("files.cmd.empty_trash"),
            true,
        ),
        D::ConfirmPurge(_) => (Some("files.cancel"), None, Some("files.cmd.purge"), true),
        D::Properties(_) | D::Failed(..) => (Some("files.close"), None, None, false),
        D::Conflict(..) => (
            Some("files.conflict.skip"),
            Some("files.conflict.keep_both"),
            Some("files.conflict.replace"),
            true,
        ),
        D::DriveBusy(_) => (Some("files.cancel"), None, Some("files.busy.retry"), false),
    };
    let mut add = |key: &str, target: Target, variant: ButtonVariant| -> Result<NodeId, UiError> {
        let id = ui.add(row, Button::new(Label::key(key)).variant(variant))?;
        Ok(b.mark(id, target))
    };
    let cancel = cancel
        .map(|k| add(k, Target::DialogCancel, ButtonVariant::Plain))
        .transpose()?;
    alt.map(|k| add(k, Target::DialogAlt, ButtonVariant::Plain))
        .transpose()?;
    let variant = if danger {
        ButtonVariant::Danger
    } else {
        ButtonVariant::Primary
    };
    let ok = ok.map(|k| add(k, Target::DialogOk, variant)).transpose()?;
    ui.set_cancel_action(id, cancel)?;
    Ok(ok.or(cancel))
}
