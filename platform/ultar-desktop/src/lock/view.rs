//! The two stages of the lock screen as widget trees (boards Lock and Login).

use super::{LockState, SignInError, Stage};
use crate::model::time::{self, LocalTime};
use crate::view::{new_ui, Look, Target, View};
use ultar_bus::messages::auth::Method;
use ultar_i18n::datefmt::ClockStyle;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{FontRole, TextAlign, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, Button, ButtonVariant, Card, CardTone, Icon, IconButton, InfoBar, Label,
    Picture, Text, TextField, Tone,
};

/// Width of the notification cards and the password field (`g(100)`, `g(84)`).
const CARD_WIDTH: f32 = 100.0 * Space::S4.points();
const FIELD_WIDTH: f32 = 84.0 * Space::S4.points();

/// Builds the screen of the current stage (`None` while hidden).
pub fn build(
    s: &LockState,
    now: (u64, Option<LocalTime>),
    look: Look,
    size: Size,
    tr: &Translator,
) -> Result<Option<View>, UiError> {
    if s.stage == Stage::Hidden {
        return Ok(None);
    }
    let mut ui = new_ui(look, size, tr)?;
    ui.set_background(ColorRole::Ground);
    ui.set_root_flex(Flex::column().padding(Padding::all(Space::S24)));
    let mut view = View::new(ui);
    let root = view.ui.root();
    let status = view
        .ui
        .add(root, Flex::row().main(MainAlign::End).gap(Space::S12))?;
    view.ui.add(
        status,
        Icon::new(IconId::Offline)
            .small()
            .color(ColorRole::TextMuted),
    )?;
    let middle = Flex::column()
        .gap(Space::S24)
        .main(MainAlign::Center)
        .cross(CrossAlign::Center);
    let middle = view.ui.add_sized(root, middle, Sizing::grow(1))?;
    match s.stage {
        Stage::Cover => cover(&mut view, middle, s, now.1, tr)?,
        _ => sign_in(&mut view, middle, s, now.0)?,
    }
    bottom(&mut view, root, s)?;
    Ok(Some(view))
}

/// Most user cards shown (the rest sign in through user switching later).
const MAX_USER_CARDS: usize = 4;

fn cover(
    view: &mut View,
    middle: NodeId,
    s: &LockState,
    t: Option<LocalTime>,
    tr: &Translator,
) -> Result<(), UiError> {
    let clock = view
        .ui
        .add(middle, Flex::column().cross(CrossAlign::Center))?;
    if let Some(t) = t {
        let time = ultar_i18n::datefmt::format_time(t.hour, t.minute, ClockStyle::H24, tr);
        let big = TextSpec::new(TypeSize::S32).weight(Weight::Bold);
        let big = TextSpec {
            role: FontRole::Title,
            ..big
        };
        view.ui.add(
            clock,
            Text::new(Label::data(&time))
                .spec(big)
                .align(TextAlign::Center),
        )?;
        let (date, hijri) = time::lock_lines(t, tr);
        view.ui.add(
            clock,
            Text::new(Label::data(&date)).spec(TextSpec::new(TypeSize::S16)),
        )?;
        let small = Text::new(Label::data(&hijri)).spec(TextSpec::new(TypeSize::S12));
        view.ui.add(clock, small.color(ColorRole::TextMuted))?;
    }
    let unlock = Button::new(Label::key("lock.unlock"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Lock);
    let id = view.ui.add(middle, unlock)?;
    view.mark(id, Target::Unlock);
    let _ = view.ui.focus_with(id, false);
    hidden_notes(view, middle, s)
}

/// Per app how many notifications wait – never their content (DESKTOP-SPEC §5.3).
fn hidden_notes(view: &mut View, middle: NodeId, s: &LockState) -> Result<(), UiError> {
    for (app, count) in s.notes.iter().take(4) {
        let card = Card {
            flex: Flex::row()
                .gap(Space::S12)
                .cross(CrossAlign::Center)
                .padding(Padding::symmetric(Space::S16, Space::S12)),
            floating: false,
            raised: false,
            tone: CardTone::Plain,
        };
        let card = view.ui.add_sized(middle, card, Sizing::width(CARD_WIDTH))?;
        let known = crate::model::apps::base_apps()
            .into_iter()
            .find(|a| &a.id == app);
        let icon = known.as_ref().map_or(IconId::Bell, |a| a.icon);
        view.ui
            .add(card, Icon::new(icon).color(ColorRole::TextMuted))?;
        let texts = view.ui.add_sized(card, Flex::column(), Sizing::grow(1))?;
        let name = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
        let label = known.map_or_else(|| Label::data(app), |a| a.label());
        view.ui.add(texts, Text::new(label).spec(name))?;
        let hidden = Label::key_with("lock.notes_hidden", &[("count", &count.to_string())]);
        let hidden = Text::new(hidden).spec(TextSpec::new(TypeSize::S12));
        view.ui.add(texts, hidden.color(ColorRole::TextMuted))?;
    }
    Ok(())
}

fn error_label(s: &LockState, now: u64) -> Option<Label> {
    let wait = s.wait_ms(now);
    match s.error? {
        SignInError::Wrong if wait > 0 => {
            let secs = wait.div_ceil(1000).to_string();
            Some(Label::key_with("lock.error.wait", &[("seconds", &secs)]))
        }
        SignInError::Wrong => Some(Label::key("lock.error.wrong")),
        SignInError::Unavailable => Some(Label::key("lock.error.unavailable")),
        SignInError::NoService => Some(Label::key("lock.error.no_service")),
        SignInError::NoUser => Some(Label::key("lock.error.no_user")),
    }
}

/// "Ultar OS" with the mark, then the accounts (several) or the one user's picture and
/// name; returns that name.
fn heading(view: &mut View, middle: NodeId, s: &LockState) -> Result<String, UiError> {
    let brand = view.ui.add(
        middle,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
    )?;
    view.ui.add(
        brand,
        Brick::new(BrickKind::Art(Picture::Mark), Label::key("lock.brand")),
    )?;
    let title = TextSpec {
        role: FontRole::Title,
        ..TextSpec::new(TypeSize::S24).weight(Weight::Bold)
    };
    view.ui
        .add(brand, Text::new(Label::key("lock.brand")).spec(title))?;
    let name = s
        .user
        .as_ref()
        .map_or(String::new(), |u| u.display_name.clone());
    if s.users.len() > 1 {
        users(view, middle, s)?;
    } else {
        let who = view.ui.add(
            middle,
            Flex::column().gap(Space::S8).cross(CrossAlign::Center),
        )?;
        view.ui.add(
            who,
            Brick::new(BrickKind::Avatar, Label::key("lock.user_picture")),
        )?;
        let strong = TextSpec::new(TypeSize::S16).weight(Weight::Semibold);
        view.ui
            .add(who, Text::new(Label::data(&name)).spec(strong))?;
    }
    Ok(name)
}

fn sign_in(view: &mut View, middle: NodeId, s: &LockState, now: u64) -> Result<(), UiError> {
    let name = heading(view, middle, s)?;
    let key = match s.method {
        Method::Password => "lock.password",
        Method::Pin => "lock.pin",
    };
    let field = TextField::password(Label::key_with(key, &[("user", &name)]))
        .labelled()
        .max_len(ultar_bus::messages::auth::MAX_SECRET);
    let field = view
        .ui
        .add_sized(middle, field, Sizing::width(FIELD_WIDTH))?;
    view.mark(field, Target::Password);
    let busy = s.stage == Stage::Checking || s.wait_ms(now) > 0 || s.user.is_none();
    view.ui.set_disabled(field, busy)?;
    view.ui.set_error(field, error_label(s, now))?;
    if !busy {
        view.ui.focus_with(field, true)?;
    }
    if s.caps_lock {
        let hint = InfoBar::new(Tone::Warning, Label::key("lock.caps_lock"));
        view.ui
            .add_sized(middle, hint, Sizing::width(FIELD_WIDTH))?;
    }
    let submit = Button::new(Label::key("lock.submit")).variant(ButtonVariant::Primary);
    let submit = view.ui.add(middle, submit)?;
    view.mark(submit, Target::Submit);
    view.ui.set_disabled(submit, busy)?;
    view.ui.set_loading(submit, s.stage == Stage::Checking)?;
    if s.user.as_ref().is_some_and(|u| u.has_pin) {
        let other = match s.method {
            Method::Password => "lock.use_pin",
            Method::Pin => "lock.use_password",
        };
        let method = Button::new(Label::key(other)).variant(ButtonVariant::Quiet);
        let method = view.ui.add(middle, method)?;
        view.mark(method, Target::Method);
    }
    Ok(())
}

/// Board Login: the accounts as cards. The session's own user is chosen; another one
/// cannot take over a locked session (that needs user switching, which has no service yet),
/// so those cards are disabled and say why.
fn users(view: &mut View, middle: NodeId, s: &LockState) -> Result<(), UiError> {
    let row = view.ui.add(middle, Flex::row().gap(Space::S12))?;
    let own = s.user.as_ref().map(|u| u.name.as_str());
    for (i, u) in s.users.iter().take(MAX_USER_CARDS).enumerate() {
        let chosen = Some(u.name.as_str()) == own;
        let card = view.ui.add(
            row,
            Brick::new(BrickKind::UserCard { chosen }, Label::data(&u.display_name)),
        )?;
        view.mark(card, Target::User(i));
        if !chosen {
            view.ui.set_disabled(card, true)?;
            view.ui
                .set_tooltip(card, Some(Label::key("lock.reason.other_user")))?;
        }
    }
    Ok(())
}

fn bottom(view: &mut View, root: NodeId, s: &LockState) -> Result<(), UiError> {
    if s.stage != Stage::Cover {
        return sign_in_tools(view, root, s);
    }
    let row = view.ui.add(
        root,
        Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Center),
    )?;
    let sos = Button::new(Label::key("lock.emergency")).icon(IconId::Sos);
    let sos = view.ui.add(row, sos)?;
    view.mark(sos, Target::Emergency);
    view.ui.set_disabled(sos, true)?;
    view.ui
        .set_tooltip(sos, Some(Label::key("lock.reason.no_emergency")))?;
    let off = Button::new(Label::key("lock.power_off")).icon(IconId::Power);
    let off = view.ui.add(row, off)?;
    view.mark(off, Target::EmergencyOff);
    view.ui.set_disabled(off, true)?;
    view.ui
        .set_tooltip(off, Some(Label::key("lock.reason.no_power")))?;
    Ok(())
}

/// Board Login: input language, accessibility and power at the end edge.
fn sign_in_tools(view: &mut View, root: NodeId, s: &LockState) -> Result<(), UiError> {
    let end = view.ui.add(
        root,
        Flex::row()
            .gap(Space::S8)
            .main(MainAlign::End)
            .cross(CrossAlign::Center),
    )?;
    let (names, active) = &s.layouts;
    if names.len() > 1 {
        let current = names.get(usize::from(*active)).cloned().unwrap_or_default();
        let chip = Brick::new(
            BrickKind::Chip,
            Label::key(format!("desk.kbd.short.{current}")),
        )
        .named(Label::key(format!("desk.kbd.name.{current}")));
        let id = view.ui.add(end, chip)?;
        view.mark(id, Target::Keyboard);
    }
    let access = view.ui.add(
        end,
        IconButton::new(IconId::Access, Label::key("lock.access")),
    )?;
    view.mark(access, Target::Access);
    view.ui.set_disabled(access, true)?;
    view.ui
        .set_tooltip(access, Some(Label::key("lock.reason.no_settings")))?;
    let power = view.ui.add(
        end,
        IconButton::new(IconId::Power, Label::key("desk.qs.power")),
    )?;
    view.mark(power, Target::EmergencyOff);
    view.ui.set_disabled(power, true)?;
    view.ui
        .set_tooltip(power, Some(Label::key("lock.reason.no_power")))?;
    Ok(())
}
