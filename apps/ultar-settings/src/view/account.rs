//! Settings > Accounts > Ultar account (board *Account*, DESKTOP-SPEC §12.1).

use super::{k, metrics, muted, section, spec, Cx};
use crate::action::Act;
use ultar_bus::messages::account_service::{AccountService, AccountState, DeviceEntry, Phase};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Badge, BarEdge, Button, ButtonVariant, Card, EmptyState, IconButton, InfoBar, Label, Progress,
    Row, Switch, Text, TextField, Tone,
};

/// Longest typed user name, password and server address in bytes.
pub const MAX_FIELD: usize = 256;

/// Catalog key stem of an account service.
pub fn service_key(s: AccountService) -> &'static str {
    match s {
        AccountService::Calls => "app.settings.account.svc.calls",
        AccountService::Vpn => "app.settings.account.svc.vpn",
        AccountService::StorePurchases => "app.settings.account.svc.store",
        AccountService::Backup => "app.settings.account.svc.backup",
        AccountService::MeshOverInternet => "app.settings.account.svc.mesh",
    }
}

/// Password strength 0–4 (length and character classes); no dictionary, only a hint.
pub fn strength(password: &str) -> u8 {
    let n = password.chars().count();
    if n == 0 {
        return 0;
    }
    let classes = [
        password.chars().any(|c| c.is_lowercase()),
        password.chars().any(|c| c.is_uppercase()),
        password.chars().any(|c| c.is_numeric()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    match (n, classes) {
        (0..=7, _) => 1,
        (8..=11, 0..=2) => 2,
        (8..=11, _) | (12.., 0..=1) => 3,
        _ => 4,
    }
}

/// Narrowest the middle column (sign-in) may be beside the other two.
const MIDDLE_MIN: f32 = 320.0;

/// The account service's key of a refused password (the only failure that uses up an attempt).
const WRONG_PASSWORD: &str = "accountd.refused.wrong_password";

fn header_row(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let row = cx.ui.add(
        content,
        Flex::row().gap(Space::S8).cross(CrossAlign::Center),
    )?;
    let back = IconButton::new(IconId::Back, k("app.settings.account.back"));
    cx.act(row, back, Act::AccountBack)?;
    cx.ui.add(
        row,
        Text::new(k("app.settings.account.title")).spec(spec::page_title()),
    )?;
    Ok(())
}

/// Builds the page.
pub fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    header_row(cx, content)?;
    let columns = cx.ui.add(
        content,
        Flex::row().gap(Space::S16).cross(CrossAlign::Start),
    )?;
    // three columns only when all of them fit (status, account, devices); else one below the other
    let gaps = 2.0 * Space::S16.points();
    let three =
        metrics::account_status_column() + metrics::account_devices_column() + MIDDLE_MIN + gaps;
    let narrow = super::shell::compact(cx.ui) || super::content_width(cx.ui) < three;
    let (a, b, c) = if narrow {
        (Sizing::grow(1), Sizing::grow(1), Sizing::grow(1))
    } else {
        (
            Sizing::width(metrics::account_status_column()),
            Sizing::grow(1),
            Sizing::width(metrics::account_devices_column()),
        )
    };
    let parent = if narrow {
        cx.ui.add(content, Flex::column().gap(Space::S16))?
    } else {
        columns
    };
    let col1 = cx.ui.add_sized(parent, Flex::column().gap(Space::S16), a)?;
    status_card(cx, col1)?;
    let col2 = cx.ui.add_sized(parent, Flex::column().gap(Space::S16), b)?;
    account_card(cx, col2)?;
    let col3 = cx.ui.add_sized(parent, Flex::column().gap(Space::S16), c)?;
    devices_card(cx, col3)
}

fn state(cx: &Cx<'_>) -> AccountState {
    cx.state.account.state.clone().unwrap_or_default()
}

fn status_card(cx: &mut Cx<'_>, col: NodeId) -> Result<(), UiError> {
    let st = state(cx);
    let signed_in = matches!(st.phase, Phase::SignedIn | Phase::Offline);
    let title = if signed_in { "signed_in" } else { "signed_out" };
    let (card, _) = section(
        cx.ui,
        col,
        Sizing::AUTO,
        k(&format!("app.settings.account.{title}")),
        Some(k("app.settings.account.purpose")),
    )?;
    let body = cx.ui.add(
        card,
        Flex::column()
            .padding(Padding::all(Space::S16))
            .gap(Space::S12),
    )?;
    status_bar(cx, body, &st)?;
    if !signed_in {
        sign_in_form(cx, body, &st)?;
    }
    let server = TextField::new(k("app.settings.account.server"))
        .labelled()
        .direction(ultar_ui::widgets::TextDirection::Ltr)
        .max_len(MAX_FIELD)
        .text(&cx.state.account.server);
    let server = cx.act(body, server, Act::AccountServer)?;
    if signed_in || cx.state.account.state.is_none() {
        cx.ui.set_disabled(server, true)?;
        cx.ui
            .set_tooltip(server, Some(k("app.settings.account.server.locked")))?;
    }
    Ok(())
}

fn status_bar(cx: &mut Cx<'_>, body: NodeId, st: &AccountState) -> Result<(), UiError> {
    let bar = if let Some(error) = &cx.state.account.error {
        Some(
            InfoBar::new(Tone::Danger, k("app.settings.account.unavailable"))
                .message(crate::i18n::label(error, cx.tr)),
        )
    } else {
        match st.phase {
            Phase::Offline => Some(
                InfoBar::new(Tone::Warning, k("app.settings.account.offline"))
                    .message(k("app.settings.account.offline.sub")),
            ),
            Phase::Locked => {
                let m = st.locked_minutes.to_string();
                Some(
                    InfoBar::new(Tone::Danger, k("app.settings.account.locked")).message(
                        Label::key_with("app.settings.account.locked.sub", &[("n", &m)]),
                    ),
                )
            }
            _ => st.error.as_ref().map(|e| {
                let reason = crate::i18n::label(e, cx.tr);
                // the attempts left only after a refused password: a server that cannot be
                // reached (or any other reason) uses none up and says only why
                let message = if e.key == WRONG_PASSWORD {
                    let remaining = st.attempts_remaining.to_string();
                    Label::key_with(
                        "app.settings.account.failed.sub",
                        &[("reason", &reason.resolve(cx.tr)), ("n", &remaining)],
                    )
                } else {
                    reason
                };
                InfoBar::new(Tone::Danger, k("app.settings.account.failed")).message(message)
            }),
        }
    };
    if let Some(bar) = bar {
        cx.ui.add(body, bar)?;
    }
    Ok(())
}

fn sign_in_form(cx: &mut Cx<'_>, body: NodeId, st: &AccountState) -> Result<(), UiError> {
    let user = TextField::new(k("app.settings.account.user"))
        .labelled()
        .direction(ultar_ui::widgets::TextDirection::Ltr)
        .max_len(MAX_FIELD)
        .text(&cx.state.account.user);
    cx.act(body, user, Act::AccountUser)?;
    // Boards: the password field and its show button beside it (bottoms aligned).
    let line = cx
        .ui
        .add(body, Flex::row().gap(Space::S8).cross(CrossAlign::End))?;
    let password = TextField::password(k("app.settings.account.password"))
        .direction(ultar_ui::widgets::TextDirection::Auto)
        .labelled()
        .reveal_outside()
        .max_len(MAX_FIELD);
    cx.act_sized(line, password, Sizing::grow(1), Act::AccountPassword)?;
    let show = IconButton::new(IconId::Eye, k("app.settings.account.show"));
    cx.act(line, show, Act::AccountShow)?;
    let meter = cx.ui.add(body, Flex::column())?;
    let text = muted(cx.ui, meter, k("app.settings.account.strength.0"))?;
    cx.view.live.push((Act::AccountPassword, text));
    // Boards: the meter is the 4-point bar alone.
    let bar = cx.ui.add_sized(
        meter,
        Progress::new(k("app.settings.account.strength")).value(0.0),
        Sizing::AUTO.with_height(Space::S4.points()),
    )?;
    cx.view.live.push((Act::AccountPassword, bar));
    let buttons = cx.ui.add(body, Flex::row().gap(Space::S8))?;
    let sign_in = Button::new(k("app.settings.account.sign_in")).variant(ButtonVariant::Primary);
    let sign_in = cx.act(buttons, sign_in, Act::AccountSignIn)?;
    if st.phase == Phase::SigningIn {
        cx.ui.set_loading(sign_in, true)?;
    }
    if st.phase == Phase::Locked || cx.state.account.state.is_none() {
        cx.ui.set_disabled(sign_in, true)?;
    }
    let create = cx.act(
        buttons,
        Button::new(k("app.settings.account.create")),
        Act::AccountCreate,
    )?;
    if cx.state.account.state.is_none() {
        cx.ui.set_disabled(create, true)?;
    }
    Ok(())
}

fn account_card(cx: &mut Cx<'_>, col: NodeId) -> Result<(), UiError> {
    let st = state(cx);
    if !matches!(st.phase, Phase::SignedIn | Phase::Offline) {
        let card = cx.ui.add(col, Card::padded())?;
        let empty = EmptyState::new(IconId::Cloud, k("app.settings.account.none"))
            .message(k("app.settings.account.none.sub"));
        cx.ui.add(card, empty)?;
        return Ok(());
    }
    let card = cx.ui.add(col, Card::new())?;
    let head = cx.ui.add(
        card,
        Card::rule(BarEdge::Bottom).flex(
            Flex::row()
                .padding(Padding::all(Space::S16))
                .gap(Space::S12)
                .cross(CrossAlign::Center),
        ),
    )?;
    // Boards: a 48-point avatar place with the cloud in its middle.
    let avatar = cx.ui.add_sized(
        head,
        Flex::row()
            .main(ultar_ui::layout::MainAlign::Center)
            .cross(CrossAlign::Center),
        Sizing::width(Space::S48.points()).with_height(Space::S48.points()),
    )?;
    cx.ui
        .add(avatar, ultar_ui::widgets::Icon::new(IconId::Cloud))?;
    let names = cx.ui.add_sized(head, Flex::column(), Sizing::grow(1))?;
    cx.ui.add(
        names,
        Text::new(Label::data(&st.user)).spec(spec::card_title()),
    )?;
    muted(cx.ui, names, Label::data(&st.server))?;
    cx.ui.add(
        head,
        Badge::new(k("app.settings.account.signed_in"))
            .tone(Tone::Success)
            .with_icon(),
    )?;
    number_block(cx, card, &st)?;
    service_rows(cx, card, &st)?;
    account_buttons(cx, card)
}

fn service_rows(cx: &mut Cx<'_>, card: NodeId, st: &AccountState) -> Result<(), UiError> {
    for s in AccountService::ALL {
        let key = service_key(s);
        let row = Row::new(k(key)).sublabel(k(&format!("{key}.sub")));
        let row = cx.ui.add(card, row)?;
        let on = st.services.contains(&s);
        let switch = cx.act(row, Switch::new(k(key), on), Act::AccountServiceSwitch(s))?;
        if st.phase == Phase::Offline {
            cx.ui.set_disabled(switch, true)?;
        }
    }
    Ok(())
}

fn account_buttons(cx: &mut Cx<'_>, card: NodeId) -> Result<(), UiError> {
    // Boards: one wrapping row under a hairline; deleting the account comes last.
    let buttons = cx.ui.add(
        card,
        Card::rule(BarEdge::Top).flex(
            Flex::row()
                .padding(Padding::all(Space::S16))
                .gap(Space::S8)
                .wrap(),
        ),
    )?;
    let change = Button::new(k("app.settings.account.change_password"));
    cx.act(buttons, change, Act::AccountChangePassword)?;
    let out = Button::new(k("app.settings.account.sign_out"));
    cx.act(buttons, out, Act::AccountSignOut)?;
    let delete = Button::new(k("app.settings.account.delete"))
        .variant(ButtonVariant::Danger)
        .icon(IconId::Trash);
    cx.act(buttons, delete, Act::AccountDelete)?;
    Ok(())
}

fn number_block(cx: &mut Cx<'_>, card: NodeId, st: &AccountState) -> Result<(), UiError> {
    let block = cx.ui.add(
        card,
        Card::rule(BarEdge::Bottom).flex(
            Flex::column()
                .padding(Padding::all(Space::S16))
                .gap(Space::S12),
        ),
    )?;
    let line = cx
        .ui
        .add(block, Flex::row().gap(Space::S16).cross(CrossAlign::Center))?;
    match ultar_ui::widgets::QrCode::new(k("app.settings.account.qr_image"), st.number.as_bytes()) {
        Ok(qr) => {
            cx.ui.add(line, qr)?;
        }
        Err(_) => {
            let icon =
                ultar_ui::widgets::Icon::new(IconId::Qr).name(k("app.settings.account.qr_image"));
            cx.ui.add(line, icon)?;
        }
    }
    let texts = cx.ui.add_sized(line, Flex::column(), Sizing::grow(1))?;
    muted(cx.ui, texts, k("app.settings.account.number"))?;
    let big = ultar_ui::paint::TextSpec::new(TypeSize::S24)
        .weight(ultar_ui::text::Weight::Bold)
        .role(ultar_ui::text::FontRole::Mono);
    cx.ui
        .add(texts, Text::new(Label::data(&st.number)).spec(big))?;
    muted(cx.ui, texts, k("app.settings.account.number.sub"))?;
    let buttons = cx.ui.add(block, Flex::row().gap(Space::S8))?;
    let copied = cx.state.account.copied;
    let copy_key = if copied {
        "app.settings.account.copied"
    } else {
        "app.settings.account.copy"
    };
    let copy = Button::new(k(copy_key)).icon(IconId::Copy);
    let copy = cx.act(buttons, copy, Act::AccountCopy)?;
    cx.ui
        .set_tooltip(copy, Some(k("app.settings.account.copy.name")))?;
    let qr = Button::new(k("app.settings.account.qr")).icon(IconId::Qr);
    cx.act(buttons, qr, Act::AccountQr)?;
    Ok(())
}

fn devices_card(cx: &mut Cx<'_>, col: NodeId) -> Result<(), UiError> {
    let st = state(cx);
    let (card, _) = section(
        cx.ui,
        col,
        Sizing::AUTO,
        k("app.settings.account.devices"),
        Some(k("app.settings.account.devices.sub")),
    )?;
    if st.devices.is_empty() {
        let body = cx
            .ui
            .add(card, Flex::column().padding(Padding::all(Space::S16)))?;
        muted(cx.ui, body, k("app.settings.account.devices.none"))?;
        return Ok(());
    }
    for (i, d) in st.devices.iter().enumerate() {
        device_item(cx, card, i, d)?;
    }
    Ok(())
}

fn device_item(cx: &mut Cx<'_>, card: NodeId, i: usize, d: &DeviceEntry) -> Result<(), UiError> {
    let item = cx.ui.add(
        card,
        Card::rule(BarEdge::Bottom).flex(
            Flex::column()
                .padding(Padding::symmetric(Space::S16, Space::S8))
                .gap(Space::S8),
        ),
    )?;
    let texts_h = super::lines_height(cx.ui, TypeSize::S14, 1.0)
        + super::lines_height(cx.ui, TypeSize::S12, 1.0);
    let line = cx.ui.add_sized(
        item,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
        Sizing::AUTO.with_height(texts_h),
    )?;
    let icon = match d.kind.as_str() {
        "phone" => IconId::Phone,
        "laptop" => IconId::Device,
        _ => IconId::Monitor,
    };
    cx.ui.add(
        line,
        ultar_ui::widgets::Icon::new(icon).color(ultar_ui::tokens::ColorRole::TextMuted),
    )?;
    let texts = cx.ui.add_sized(line, Flex::column(), Sizing::grow(1))?;
    cx.ui
        .add(texts, Text::new(Label::data(&d.name)).spec(spec::label()))?;
    let kind = format!("app.settings.account.kind.{}", d.kind);
    let kind = if cx.tr.has(&kind) {
        kind
    } else {
        "app.settings.account.kind.device".into()
    };
    let kind = cx.tr.t(&kind).into_owned();
    let seen = if d.last_seen == 0 {
        Label::key_with("app.settings.account.active_now", &[("kind", &kind)])
    } else {
        Label::key_with("app.settings.account.last_seen", &[("kind", &kind)])
    };
    muted(cx.ui, texts, seen)?;
    device_actions(cx, item, i, d)
}

/// The actions of a device of the account (or the "this device" badge), at the end.
fn device_actions(cx: &mut Cx<'_>, item: NodeId, i: usize, d: &DeviceEntry) -> Result<(), UiError> {
    // the badge: 24 high plus its two borders (CSS content box)
    let actions_h = if d.this_device { 26.0 } else { 32.0 };
    let actions = cx.ui.add_sized(
        item,
        Flex::row()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .main(ultar_ui::layout::MainAlign::End),
        Sizing::AUTO.with_height(actions_h),
    )?;
    if d.this_device {
        let badge = Badge::new(k("app.settings.account.this_device"))
            .tone(Tone::Success)
            .with_icon();
        cx.ui.add(actions, badge)?;
        return Ok(());
    }
    let out = Button::new(k("app.settings.account.device.sign_out")).variant(ButtonVariant::Quiet);
    let out = cx.act(actions, out, Act::AccountDeviceSignOut(i))?;
    let name = Label::key_with(
        "app.settings.account.device.sign_out.name",
        &[("name", &d.name)],
    );
    cx.ui.set_accessible_name(out, Some(name))?;
    let remove =
        Button::new(k("app.settings.account.device.remove")).variant(ButtonVariant::Danger);
    let remove = cx.act(actions, remove, Act::AccountDeviceRemove(i))?;
    let name = Label::key_with(
        "app.settings.account.device.remove.name",
        &[("name", &d.name)],
    );
    cx.ui.set_accessible_name(remove, Some(name))?;
    Ok(())
}

/// The page "Create account": user name, password twice, optional e-mail.
pub fn create(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    header_row(cx, content)?;
    let card = cx.ui.add(content, Card::padded())?;
    cx.ui.add(
        card,
        Text::new(k("app.settings.account.create.title")).spec(spec::card_title()),
    )?;
    let fields = [
        ("app.settings.account.user", false),
        ("app.settings.account.password", true),
        ("app.settings.account.password_again", true),
        ("app.settings.account.email", false),
    ];
    for (i, (key, secret)) in fields.iter().enumerate() {
        let field = if *secret {
            TextField::password(k(key))
        } else {
            TextField::new(k(key)).direction(ultar_ui::widgets::TextDirection::Ltr)
        };
        let id = cx.act(
            card,
            field.labelled().max_len(MAX_FIELD),
            Act::DialogField(i),
        )?;
        cx.view.dialog_fields.push(id);
    }
    if let Some(error) = cx.state.dialog.as_ref().and_then(|d| d.error.clone()) {
        cx.ui.add(
            card,
            InfoBar::new(Tone::Danger, crate::i18n::label(&error, cx.tr)),
        )?;
    }
    let buttons = cx.ui.add(card, Flex::row().gap(Space::S8))?;
    let ok = Button::new(k("app.settings.account.create")).variant(ButtonVariant::Primary);
    cx.act(buttons, ok, Act::DialogOk)?;
    cx.act(
        buttons,
        Button::new(k("app.settings.cancel")),
        Act::DialogCancel,
    )?;
    Ok(())
}
