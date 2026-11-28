//! The pages without a board of their own (steps 1–6, 9, 12, 13, 15, 16) and step 8
//! (components, board *Installer*).

use super::{body_muted, k, muted, Cx};
use crate::action::Act;
use crate::components::{Component, Scope};
use crate::wizard::{Step, KEYBOARDS, TIMEZONES};
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Badge, Button, ButtonVariant, Card, Checkbox, InfoBar, Label, Progress, RadioGroup,
    RadioLayout, Row, Select, Switch, Text, TextDirection, TextField, Tone,
};

/// Title and subtitle of a page.
pub(crate) fn header(cx: &mut Cx<'_>, content: NodeId, step: Step) -> Result<(), UiError> {
    let n = step.page().number();
    let col = cx.ui.add(content, Flex::column())?;
    let title = TextSpec::title(TypeSize::S24);
    cx.ui.add(
        col,
        Text::new(k(&format!("installer.page.{n}"))).spec(title),
    )?;
    body_muted(cx.ui, col, k(&format!("installer.page.{n}.sub")))?;
    Ok(())
}

/// Builds the page of the shown step.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let step = cx.model.wizard.step.page();
    if !matches!(step, Step::Drivers) {
        header(cx, content, step)?;
    }
    match step {
        Step::Components => components(cx, content),
        Step::Disk | Step::Encryption => super::disk::build(cx, content),
        Step::Drivers => super::drivers::build(cx, content),
        Step::Summary => super::summary::build(cx, content),
        Step::Install | Step::Done => progress(cx, content, step),
        _ => simple(cx, content, step),
    }
}

/// The welcome on step 1 (BRAND.md §4): the mark 96, the greeting, the company line.
fn welcome(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let head = cx.ui.add(
        content,
        Flex::column()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .padding(Padding::symmetric(Space::S16, Space::S16)),
    )?;
    let mark = ultar_ui::widgets::Icon::new(ultar_ui::icon::IconId::Mark)
        .sized(96)
        .color(ColorRole::AccentText)
        .name(k("brand.label.mark"));
    cx.ui.add(head, mark)?;
    cx.ui.add(
        head,
        Text::new(k("installer.welcome")).spec(TextSpec::title(TypeSize::S20)),
    )?;
    logo(cx, head)?;
    Ok(())
}

/// Width of the company logo on the installer's pages (points).
const LOGO_WIDTH: u16 = 240;

/// The company logo (BRAND.md §4: installer welcome and the last page), centred.
fn logo(cx: &mut Cx<'_>, parent: NodeId) -> Result<(), UiError> {
    let logo = ultar_ui::widgets::Brick::new(
        ultar_ui::widgets::BrickKind::Logo {
            width: LOGO_WIDTH,
            quiet: false,
        },
        k("brand.label.logo"),
    );
    cx.ui.add(parent, logo)?;
    Ok(())
}

fn choices(keys: &[&str]) -> Vec<Label> {
    keys.iter().map(|key| k(key)).collect()
}

fn select_row(
    cx: &mut Cx<'_>,
    card: NodeId,
    name: &str,
    opts: Vec<Label>,
    at: usize,
    act: Act,
) -> Result<(), UiError> {
    let row = cx.ui.add(card, Row::new(k(name)).last())?;
    cx.act(row, Select::new(k(name), opts).selected(at), act)?;
    Ok(())
}

fn switch_rows(
    cx: &mut Cx<'_>,
    card: NodeId,
    keys: [&str; 2],
    on: [bool; 2],
    act: fn(usize) -> Act,
) -> Result<(), UiError> {
    for (i, key) in keys.iter().enumerate() {
        let mut row = Row::new(k(key)).sublabel(k(&format!("{key}.sub")));
        if i == 1 {
            row = row.last();
        }
        let row = cx.ui.add(card, row)?;
        cx.act(row, Switch::new(k(key), on[i]), act(i))?;
    }
    Ok(())
}

fn simple(cx: &mut Cx<'_>, content: NodeId, step: Step) -> Result<(), UiError> {
    let p = cx.model.wizard.prefs.clone();
    if step == Step::Language {
        welcome(cx, content)?;
    }
    let card = cx.ui.add(content, Card::new())?;
    match step {
        Step::Language => {
            let opts = choices(&[
                "installer.lang.ar",
                "installer.lang.en",
                "installer.lang.de",
            ]);
            select_row(cx, card, "installer.lang", opts, p.lang, Act::Lang)
        }
        Step::Access => switch_rows(
            cx,
            card,
            ["installer.access.large", "installer.access.reader"],
            p.access,
            Act::Access,
        ),
        Step::Keyboard => {
            let opts = KEYBOARDS
                .iter()
                .map(|id| k(&format!("installer.keyboard.{}", id.replace('+', "_"))))
                .collect();
            select_row(
                cx,
                card,
                "installer.keyboard",
                opts,
                p.keyboard,
                Act::Keyboard,
            )
        }
        Step::Network => {
            let body = cx
                .ui
                .add(card, Flex::column().padding(Padding::all(Space::S16)))?;
            let bar = InfoBar::new(Tone::Info, k("installer.network.none"))
                .message(k("installer.network.none.sub"));
            cx.ui.add(body, bar).map(|_| ())
        }
        _ => simple_rest(cx, card, step, &p),
    }
}

fn simple_rest(
    cx: &mut Cx<'_>,
    card: NodeId,
    step: Step,
    p: &crate::wizard::Prefs,
) -> Result<(), UiError> {
    match step {
        Step::TryOrInstall => {
            let opts = ["installer.try.try", "installer.try.install"];
            radio(
                cx,
                card,
                "installer.try",
                &opts,
                usize::from(p.install),
                Act::TryOrInstall,
            )
        }
        Step::Kind => {
            let opts = ["installer.kind.desktop", "installer.kind.server"];
            radio(
                cx,
                card,
                "installer.kind",
                &opts,
                usize::from(p.server),
                Act::Kind,
            )
        }
        Step::Extras => switch_rows(
            cx,
            card,
            ["installer.extra.updates", "installer.extra.codecs"],
            p.extras,
            Act::Extra,
        ),
        Step::Account => account(cx, card),
        Step::TimeZone => {
            let opts = TIMEZONES.iter().map(|z| Label::data(z)).collect();
            select_row(
                cx,
                card,
                "installer.timezone",
                opts,
                p.timezone,
                Act::TimeZone,
            )
        }
        _ => Ok(()),
    }
}

fn radio(
    cx: &mut Cx<'_>,
    card: NodeId,
    name: &str,
    keys: &[&str],
    selected: usize,
    act: Act,
) -> Result<(), UiError> {
    let body = cx
        .ui
        .add(card, Flex::column().padding(Padding::all(Space::S16)))?;
    cx.act(
        body,
        RadioGroup::new(k(name), choices(keys)).selected(selected),
        act,
    )?;
    Ok(())
}

fn account(cx: &mut Cx<'_>, card: NodeId) -> Result<(), UiError> {
    let body = cx.ui.add(
        card,
        Flex::column()
            .padding(Padding::all(Space::S16))
            .gap(Space::S12),
    )?;
    let keys = [
        "installer.account.full",
        "installer.account.user",
        "installer.account.device",
    ];
    for (i, key) in keys.iter().enumerate() {
        let mut f = TextField::new(k(key))
            .labelled()
            .max_len(crate::wizard::MAX_NAME * 4);
        if i > 0 {
            f = f.direction(TextDirection::Ltr);
        }
        let f = f.text(&cx.model.wizard.account[i]);
        cx.act(body, f, Act::AccountField(i))?;
        if i == 1 {
            // The password of the user: its Argon2id hash goes to the first start.
            for (n, key) in [
                "installer.account.password",
                "installer.account.password.again",
            ]
            .iter()
            .enumerate()
            {
                let p = TextField::password(k(key))
                    .labelled()
                    .text(cx.model.wizard.secrets.password[n].expose_for_field())
                    .max_len(crate::wizard::MAX_PASSWORD);
                cx.act(body, p, Act::Password(n))?;
            }
        }
    }
    muted(cx.ui, body, k("installer.account.note"))?;
    // The PC's public key for SSH (optional): the server edition lets it in at once.
    let f = TextField::new(k("installer.account.ssh_key"))
        .labelled()
        .direction(TextDirection::Ltr)
        .max_len(crate::engine::answers::MAX_VALUE)
        .text(&cx.model.wizard.account[3]);
    cx.act(body, f, Act::AccountField(3))?;
    muted(cx.ui, body, k("installer.account.ssh_key.note"))?;
    Ok(())
}

fn progress(cx: &mut Cx<'_>, content: NodeId, step: Step) -> Result<(), UiError> {
    let card = cx.ui.add(content, Card::padded())?;
    if let Some(failure) = &cx.model.failure {
        let bar = InfoBar::new(Tone::Danger, k("installer.failed"))
            .message(crate::i18n::label(failure, cx.tr));
        cx.ui.add(card, bar)?;
        return Ok(());
    }
    // The company logo while the system is written and on the last page.
    let head = cx.ui.add(
        card,
        Flex::column()
            .cross(CrossAlign::Center)
            .padding(Padding::symmetric(Space::S16, Space::S8)),
    )?;
    logo(cx, head)?;
    if step == Step::Done {
        cx.ui.add(
            card,
            InfoBar::new(Tone::Success, k("installer.done")).message(k("installer.done.sub")),
        )?;
        let restart = Button::new(k("installer.restart")).variant(ButtonVariant::Primary);
        cx.act(card, restart, Act::Restart)?;
        return Ok(());
    }
    let (done, total) = cx.model.progress;
    let value = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    let bar = cx
        .ui
        .add(card, Progress::new(k("installer.progress")).value(value))?;
    cx.view.progress = Some(bar);
    muted(cx.ui, card, k("installer.progress.note"))?;
    Ok(())
}

/// Step 8: scope cards and the component grid.
fn components(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let w = cx.model.wizard.clone();
    let scopes = Scope::ALL
        .iter()
        .map(|s| k(&format!("installer.scope.{}", s.id())))
        .collect();
    let index = Scope::ALL.iter().position(|s| *s == w.scope).unwrap_or(2);
    let mut group = RadioGroup::new(k("installer.scope"), scopes)
        .selected(index)
        .layout(RadioLayout::CardRow);
    for (i, s) in Scope::ALL.iter().enumerate() {
        group = group.detail(i, k(&format!("installer.scope.{}.sub", s.id())));
    }
    cx.act(content, group, Act::Scope)?;
    // Boards: a card with a two-column grid (gap 4 / 16, padding 8) of check rows.
    let card = cx.ui.add(
        content,
        Card::new().flex(
            Flex::column()
                .padding(Padding::all(Space::S8))
                .gap(Space::S4),
        ),
    )?;
    for pair in Component::ALL.chunks(2) {
        let line = cx
            .ui
            .add(card, Flex::row().gap(Space::S16).cross(CrossAlign::Stretch))?;
        for c in pair {
            component(cx, line, *c, w.components.contains(c))?;
        }
    }
    Ok(())
}

fn component(cx: &mut Cx<'_>, line: NodeId, c: Component, on: bool) -> Result<(), UiError> {
    let key = format!("installer.component.{}", c.id());
    let check = Checkbox::new(k(&key), on)
        .sublabel(k(&format!("{key}.sub")))
        .icon(c.icon());
    let row = cx.ui.add_sized(line, check, Sizing::grow(1))?;
    cx.view.registry.add(row, Act::Component(c));
    let end = cx
        .ui
        .add(row, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let badge = match c {
        Component::Assistant => Some("installer.component.recommended"),
        Component::Linux => Some("installer.component.needed_by_firefox"),
        _ => None,
    };
    if let Some(badge) = badge {
        cx.ui
            .add(end, Badge::new(k(badge)).tone(Tone::Info).with_icon())?;
    }
    let size = cx.ui.add(
        end,
        Text::new(Label::data("—"))
            .spec(TextSpec::new(TypeSize::S12))
            .color(ColorRole::TextMuted),
    )?;
    cx.ui
        .set_accessible_name(size, Some(k("installer.component.size")))?;
    Ok(())
}
