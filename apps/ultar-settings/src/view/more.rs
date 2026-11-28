//! What the Users, Apps, Storage and Updates pages show above their settingsd rows: the
//! local users of the account service, the installed apps with their permissions, the
//! storage and the mounted drives with how full they are, and the updater's state with "Check now" and "Update from a drive".

use super::{k, section, Cx};
use crate::action::Act;
use crate::i18n::label;
use ultar_bus::messages::account_local::UserKind;
use ultar_bus::Text as BusText;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, InfoBar, Label, Progress, Row, Switch, Text, Tone};

/// Builds the service part of the page of settingsd panel `panel` (nothing for the others).
pub fn top(cx: &mut Cx<'_>, content: NodeId, panel: &str) -> Result<(), UiError> {
    match panel {
        "users" => users(cx, content),
        "apps" => apps(cx, content),
        "storage" => drives(cx, content),
        "updates" => updates(cx, content),
        _ => Ok(()),
    }
}

/// A warning that a service could not be read.
fn unread(cx: &mut Cx<'_>, content: NodeId, error: &BusText) -> Result<(), UiError> {
    let bar = InfoBar::new(Tone::Warning, k("app.settings.status.unavailable"))
        .message(label(error, cx.tr));
    cx.ui.add(content, bar)?;
    Ok(())
}

/// A card with a title; a row saying `empty` when `count` is 0.
fn list_card(
    cx: &mut Cx<'_>,
    content: NodeId,
    title: &str,
    sub: &str,
    (count, empty): (usize, &str),
) -> Result<(NodeId, NodeId), UiError> {
    let (card, header) = section(cx.ui, content, Sizing::AUTO, k(title), Some(k(sub)))?;
    if count == 0 {
        cx.ui.add(card, Row::new(k(empty)).last())?;
    }
    Ok((card, header))
}

fn buttons(cx: &mut Cx<'_>, row: NodeId) -> Result<NodeId, UiError> {
    cx.ui
        .add(row, Flex::row().gap(Space::S8).cross(CrossAlign::Center))
}

fn users(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let more = cx.state.more.clone();
    if let Some(error) = &more.users_error {
        return unread(cx, content, error);
    }
    let (card, header) = list_card(
        cx,
        content,
        "app.settings.users.title",
        "app.settings.users.sub",
        (more.users.len(), "app.settings.users.none"),
    )?;
    let add = Button::new(k("app.settings.users.add")).variant(ButtonVariant::Primary);
    cx.act(header, add, Act::UserAdd)?;
    let count = more.users.len();
    for (n, user) in more.users.iter().enumerate() {
        let kind = match user.kind {
            UserKind::Administrator => "app.settings.users.kind.admin",
            UserKind::Standard => "app.settings.users.kind.standard",
        };
        let mut row = Row::new(Label::data(&user.display_name)).sublabel(k(kind));
        if n + 1 == count {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        let end = buttons(cx, row)?;
        let who = [("name", user.display_name.as_str())];
        let password = Button::new(k("app.settings.users.password"));
        let password = cx.act(end, password, Act::UserPassword(n))?;
        let name = Label::key_with("app.settings.users.password.name", &who);
        cx.ui.set_accessible_name(password, Some(name))?;
        let remove = Button::new(k("app.settings.users.remove")).variant(ButtonVariant::Danger);
        let remove = cx.act(end, remove, Act::UserRemove(n))?;
        let name = Label::key_with("app.settings.users.remove.name", &who);
        cx.ui.set_accessible_name(remove, Some(name))?;
        if user.current {
            cx.ui.set_disabled(remove, true)?;
            cx.ui
                .set_tooltip(remove, Some(k("app.settings.users.remove.current")))?;
        }
    }
    Ok(())
}

fn apps(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let more = cx.state.more.clone();
    if let Some(error) = &more.apps_error {
        return unread(cx, content, error);
    }
    let (card, _) = list_card(
        cx,
        content,
        "app.settings.apps.title",
        "app.settings.apps.sub",
        (more.apps.len(), "app.settings.apps.none"),
    )?;
    let rows: usize = more.apps.iter().map(|a| a.permissions.len().max(1)).sum();
    let mut shown = 0;
    for (a, app) in more.apps.iter().enumerate() {
        if app.permissions.is_empty() {
            shown += 1;
            let mut row = Row::new(Label::data(&app.id)).sublabel(Label::data(&app.version));
            if shown == rows {
                row = row.last();
            }
            let row = cx.ui.add(card, row)?;
            cx.ui
                .add(row, Text::new(k("app.settings.apps.no_permissions")))?;
        }
        for (p, (permission, granted)) in app.permissions.iter().enumerate() {
            shown += 1;
            let mut row = Row::new(Label::data(&app.id)).sublabel(Label::data(permission));
            if shown == rows {
                row = row.last();
            }
            let row = cx.ui.add(card, row)?;
            let args = [
                ("app", app.id.as_str()),
                ("permission", permission.as_str()),
            ];
            let name = Label::key_with("app.settings.apps.permission.name", &args);
            let switch = cx.act(row, Switch::new(name, *granted), Act::AppRevoke(a, p))?;
            if !*granted {
                // A permission is granted only when the app asks, in the system dialog.
                cx.ui.set_disabled(switch, true)?;
                cx.ui
                    .set_tooltip(switch, Some(k("app.settings.apps.grant_note")))?;
            }
        }
    }
    Ok(())
}

fn drives(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let more = cx.state.more.clone();
    if let Some(error) = &more.drives_error {
        return unread(cx, content, error);
    }
    let (card, _) = list_card(
        cx,
        content,
        "app.settings.storage.title",
        "app.settings.storage.sub",
        (more.drives.len(), "app.settings.storage.none"),
    )?;
    let count = more.drives.len();
    for (n, drive) in more.drives.iter().enumerate() {
        let name = if drive.name.is_empty() {
            k("app.settings.storage.internal")
        } else {
            Label::data(&drive.name)
        };
        let used = drive.total_bytes.saturating_sub(drive.free_bytes);
        let sizes = [drive.free_bytes, drive.total_bytes, used]
            .map(|bytes| ultar_i18n::units::format_size(bytes, cx.tr));
        let (free, total, used_text) = (sizes[0].as_str(), sizes[1].as_str(), sizes[2].as_str());
        let usage = Label::key_with(
            "app.settings.storage.usage",
            &[("free", free), ("total", total)],
        );
        let mut row = Row::new(name).sublabel(usage);
        if n + 1 == count {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        let share = if drive.total_bytes == 0 {
            0.0
        } else {
            used as f32 / drive.total_bytes as f32
        };
        let bar = Progress::new(Label::key_with(
            "app.settings.storage.used",
            &[("used", used_text), ("total", total)],
        ))
        .value(share);
        cx.ui.add(row, bar)?;
    }
    Ok(())
}

fn updates(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.updates.title"),
        None,
    )?;
    let state = match &cx.state.drivers.firmware {
        Some((_, Some(error))) => k(error),
        Some((phase, None)) => k(phase),
        None => k("app.settings.updates.sub"),
    };
    let row = Row::new(k("app.settings.updates.state"))
        .sublabel(state)
        .last();
    let row = cx.ui.add(card, row)?;
    let end = buttons(cx, row)?;
    // The next step of the release the check found: download it, then install it (a restart
    // from the power menu starts it; the state line says so). That step is the primary button.
    let phase = cx.state.drivers.firmware.as_ref().map(|(p, _)| p.as_str());
    let next = match phase {
        Some("update.state.available" | "update.state.paused") => {
            Some((k("app.settings.updates.download"), Act::UpdateDownload))
        }
        Some("update.state.staged") => {
            Some((k("app.settings.updates.install"), Act::UpdateInstall))
        }
        _ => None,
    };
    let check = Button::new(k("app.settings.updates.check"));
    let check = if next.is_some() {
        check
    } else {
        check.variant(ButtonVariant::Primary)
    };
    cx.act(end, check, Act::FirmwareCheck)?;
    cx.act(
        end,
        Button::new(k("app.settings.updates.usb")),
        Act::FirmwareUsb,
    )?;
    if let Some((label, act)) = next {
        cx.act(end, Button::new(label).variant(ButtonVariant::Primary), act)?;
    }
    Ok(())
}
