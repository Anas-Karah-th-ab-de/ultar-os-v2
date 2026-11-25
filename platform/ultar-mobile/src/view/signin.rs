//! The sign-in of the phone's lock screen: who signs in, the password or PIN field with its
//! error (and the wait after wrong tries), sign in, password ↔ PIN, back to the cover. The
//! decisions (who may unlock, how long to wait) are the shared lock state's
//! (`ultar-lock-core`); this file only shows them.

use super::Builder;
use crate::click::Click;
use ultar_lock_core::{LockState, SignInError, Stage};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, Button, ButtonVariant, IconButton, InfoBar, Label, Text, TextField, Tone,
};

/// Longest secret the field takes (the authentication service's limit).
pub const MAX_SECRET: usize = ultar_bus::messages::auth::MAX_SECRET;

/// The error under the field, with the seconds left when a wait runs.
pub fn error_label(s: &LockState, now_ms: u64) -> Option<Label> {
    let wait = s.wait_ms(now_ms);
    if wait > 0 {
        let seconds = wait.div_ceil(1000).to_string();
        return Some(Label::key_with(
            "mobile.signin.wait",
            &[("seconds", &seconds)],
        ));
    }
    let key = match s.error? {
        SignInError::Wrong => "mobile.signin.wrong",
        SignInError::Unavailable => "mobile.signin.unavailable",
        SignInError::NoService => "mobile.signin.no_service",
        SignInError::NoUser => "mobile.signin.no_user",
        SignInError::OtherUser => "mobile.signin.other_user",
    };
    Some(Label::key(key))
}

/// Builds the sign-in.
pub fn body(b: &mut Builder<'_>, root: NodeId, s: &LockState, now_ms: u64) -> Result<(), UiError> {
    let top = b.flex(
        root,
        Flex::row().padding(Padding::all(Space::S8)),
        Sizing::AUTO,
    )?;
    let back = IconButton::new(IconId::Back, Label::key("mobile.signin.back"));
    b.click(top, back, Sizing::AUTO, Click::SignInBack)?;
    let main = Flex::column()
        .cross(CrossAlign::Center)
        .gap(Space::S16)
        .padding(Padding::symmetric(Space::S24, Space::S32));
    let main = b.flex(root, main, Sizing::grow(1))?;
    let avatar = Brick::new(BrickKind::Avatar, Label::key("mobile.signin.picture"));
    b.add(main, avatar, Sizing::AUTO)?;
    let name = s
        .user
        .as_ref()
        .map_or(String::new(), |u| u.display_name.clone());
    let strong = TextSpec::new(TypeSize::S20).weight(Weight::Semibold);
    b.add(
        main,
        Text::new(Label::data(&name)).spec(strong),
        Sizing::AUTO,
    )?;
    let pin = s.method == ultar_bus::messages::auth::Method::Pin;
    let key = if pin {
        "mobile.signin.pin"
    } else {
        "mobile.signin.password"
    };
    let field = TextField::password(Label::key_with(key, &[("user", &name)]))
        .placeholder(Label::key(key_hint(pin)))
        .max_len(MAX_SECRET);
    let width = b.screen().w - 2.0 * Space::S24.points();
    let field = b.add(main, field, Sizing::width(width))?;
    b.built.password = Some(field);
    let busy = s.stage == Stage::Checking || s.wait_ms(now_ms) > 0 || s.user.is_none();
    b.ui.set_disabled(field, busy)?;
    b.ui.set_error(field, error_label(s, now_ms))?;
    if !busy {
        b.ui.focus(field)?;
    }
    actions(b, main, s, busy, width)
}

/// Caps Lock hint, unlock, and password ↔ PIN.
fn actions(
    b: &mut Builder<'_>,
    main: NodeId,
    s: &LockState,
    busy: bool,
    width: f32,
) -> Result<(), UiError> {
    let pin = s.method == ultar_bus::messages::auth::Method::Pin;
    if s.caps_lock {
        let hint = InfoBar::new(Tone::Warning, Label::key("mobile.signin.caps"));
        b.add(main, hint, Sizing::width(width))?;
    }
    let submit = Button::new(Label::key("mobile.signin.submit")).variant(ButtonVariant::Primary);
    let submit = b.click(main, submit, Sizing::width(width), Click::Submit)?;
    b.ui.set_disabled(submit, busy)?;
    b.ui.set_loading(submit, s.stage == Stage::Checking)?;
    if s.user.as_ref().is_some_and(|u| u.has_pin) {
        let other = if pin {
            "mobile.signin.use_password"
        } else {
            "mobile.signin.use_pin"
        };
        let method = Button::new(Label::key(other)).variant(ButtonVariant::Quiet);
        b.click(main, method, Sizing::AUTO, Click::Method)?;
    }
    Ok(())
}

fn key_hint(pin: bool) -> &'static str {
    if pin {
        "mobile.signin.pin_hint"
    } else {
        "mobile.signin.password_hint"
    }
}
