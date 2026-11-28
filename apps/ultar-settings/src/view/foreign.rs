//! Settings › Apps from other systems (`docs/FREMD-APPS.md`): the Linux area (installed?
//! version, size, update), the apps that came from other systems, and the page of one app
//! with what it may use and its removal. Names, versions and packages are data written by
//! others; nothing of an app is checked by KM Code Craft, and the page says so.

use super::{k, muted, section, title, Cx};
use crate::action::Act;
use crate::i18n::label;
use crate::state::foreign::FOLDERS;
use ultar_bus::messages::foreign::{ForeignApp, NetworkChoice, RuntimeStateReply, Source};
use ultar_ui::icon::IconId;
use ultar_ui::layout::Sizing;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, InfoBar, Label, List, ListItem, Row, Switch, Text, Tone,
};

/// Most apps shown in the list.
pub const MAX_SHOWN: usize = 64;
/// Longest name shown (characters).
const MAX_NAME: usize = 48;

/// Key of a source's kind ("Linux program").
pub fn kind_key(source: Source) -> &'static str {
    match source {
        Source::Deb => "app.settings.foreign.kind.linux",
        Source::Exe | Source::Msi => "app.settings.foreign.kind.windows",
        Source::Apk => "app.settings.foreign.kind.android",
        Source::Unknown => "app.settings.foreign.kind.unknown",
    }
}

fn folder_key(word: &str) -> String {
    format!("app.settings.foreign.folder.{word}")
}

fn name(text: &str) -> Label {
    let shown: String = text.trim().chars().take(MAX_NAME).collect();
    Label::data(&shown)
}

fn unavailable(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let error = cx.state.status.foreign.error.clone();
    let error = error.unwrap_or_else(|| ultar_bus::Text::key("app.settings.error.unavailable"));
    let bar = InfoBar::new(Tone::Warning, k("app.settings.foreign.unavailable"))
        .message(label(&error, cx.tr));
    cx.ui.add(content, bar).map(|_| ())
}

fn runtime(cx: &mut Cx<'_>, content: NodeId, r: &RuntimeStateReply) -> Result<(), UiError> {
    let (card, header) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.foreign.runtime"),
        Some(k("app.settings.foreign.runtime.sub")),
    )?;
    if !r.installed {
        // the service looks for the signed component on drives and in the areas (where
        // the Ultar Store puts its downloads) and asks an administrator
        let install = Button::new(k("app.settings.foreign.install")).icon(IconId::Package);
        cx.act(header, install, Act::ForeignUpdate)?;
        let row = Row::new(k("app.settings.foreign.runtime.none"))
            .sublabel(k("app.settings.foreign.runtime.none.sub"))
            .last();
        cx.ui.add(card, row)?;
        return Ok(());
    }
    let update = Button::new(k("app.settings.foreign.update")).icon(IconId::Update);
    cx.act(header, update, Act::ForeignUpdate)?;
    let size = ultar_i18n::units::format_size(r.size_bytes, cx.tr);
    let rows = [
        (
            "app.settings.foreign.runtime.version",
            Label::data(&r.version),
        ),
        ("app.settings.foreign.runtime.size", Label::data(&size)),
    ];
    for (i, (key, value)) in rows.into_iter().enumerate() {
        let mut row = Row::new(k(key));
        if i == 1 {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        cx.ui.add(row, Text::new(value))?;
    }
    Ok(())
}

fn apps(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let apps: Vec<ForeignApp> = cx.state.status.foreign.apps.clone();
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.foreign.apps"),
        Some(k("app.settings.foreign.apps.sub")),
    )?;
    if apps.is_empty() {
        let row = cx
            .ui
            .add(card, Row::new(k("app.settings.foreign.none")).last())?;
        return muted(cx.ui, row, k("app.settings.foreign.none.sub")).map(|_| ());
    }
    let list = cx
        .ui
        .add(card, List::new().name(k("app.settings.foreign.apps")))?;
    for (i, a) in apps.iter().take(MAX_SHOWN).enumerate() {
        let kind = k(kind_key(a.source)).resolve(cx.tr).into_owned();
        let sub = Label::key_with(
            "app.settings.foreign.app.sub",
            &[("kind", &kind), ("version", &a.version)],
        );
        let item = ListItem::new(name(&a.name))
            .icon(IconId::Package)
            .sublabel(sub)
            .chevron();
        cx.act(list, item, Act::ForeignApp(i))?;
    }
    Ok(())
}

/// The page.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    title(cx, content, k("app.settings.area.foreign"))?;
    muted(cx.ui, content, k("app.settings.foreign.intro"))?;
    let Some(r) = cx.state.status.foreign.runtime.clone() else {
        return unavailable(cx, content);
    };
    runtime(cx, content, &r)?;
    apps(cx, content)
}

/// The page of the open app.
pub(crate) fn app(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let Some(a) = cx.state.status.foreign.app().cloned() else {
        return build(cx, content);
    };
    let head = cx.ui.add(
        content,
        ultar_ui::layout::Flex::row().gap(ultar_ui::tokens::Space::S8),
    )?;
    let back = Button::new(k("app.settings.foreign.back")).icon(IconId::Back);
    cx.act(head, back, Act::ForeignBack)?;
    cx.ui.add(
        content,
        Text::new(name(&a.name)).spec(super::spec::page_title()),
    )?;
    let kind = k(kind_key(a.source)).resolve(cx.tr).into_owned();
    let sub = Label::key_with(
        "app.settings.foreign.app.from",
        &[
            ("kind", &kind),
            ("package", &a.package),
            ("version", &a.version),
        ],
    );
    muted(cx.ui, content, sub)?;
    let warning = InfoBar::new(Tone::Warning, k("app.settings.foreign.unchecked"))
        .message(k("app.settings.foreign.unchecked.sub"));
    cx.ui.add(content, warning)?;
    rights(cx, content, &a)?;
    removal(cx, content)
}

fn rights(cx: &mut Cx<'_>, content: NodeId, a: &ForeignApp) -> Result<(), UiError> {
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.foreign.rights"),
        Some(k("app.settings.foreign.rights.sub")),
    )?;
    let net = a.choices.network == NetworkChoice::Internet;
    let mut toggles = vec![(
        "app.settings.foreign.internet".to_owned(),
        net,
        Act::ForeignInternet,
    )];
    if net {
        toggles.push((
            "app.settings.foreign.lan".into(),
            a.choices.lan,
            Act::ForeignLan,
        ));
    }
    for (i, word) in FOLDERS.iter().enumerate() {
        let on = a.choices.folders.iter().any(|f| f == word);
        toggles.push((folder_key(word), on, Act::ForeignFolder(i)));
    }
    let count = toggles.len();
    for (i, (key, on, act)) in toggles.into_iter().enumerate() {
        let mut row = Row::new(k(&key)).sublabel(k(&format!("{key}.sub")));
        if i + 1 == count {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        cx.act(row, Switch::new(k(&key), on), act)?;
    }
    Ok(())
}

fn removal(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let card = cx.ui.add_sized(content, Card::new(), Sizing::AUTO)?;
    let keep = Row::new(k("app.settings.foreign.keep"))
        .sublabel(k("app.settings.foreign.keep.sub"))
        .last();
    let row = cx.ui.add(card, keep)?;
    let on = cx.state.status.foreign.keep_data;
    cx.act(
        row,
        Switch::new(k("app.settings.foreign.keep"), on),
        Act::ForeignKeep,
    )?;
    let remove = Button::new(k("app.settings.foreign.remove")).variant(ButtonVariant::Danger);
    cx.act_sized(content, remove, Sizing::AUTO, Act::ForeignRemove)
        .map(|_| ())
}
