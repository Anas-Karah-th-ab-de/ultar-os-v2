//! Boards PhoneIncoming and PhoneInCall: the ringing call, the call in progress with its
//! controls, a second (waiting or held) call and the in-call dial pad.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use super::parts::{heading, mono, small};
use super::{metrics, section, Builder};
use crate::click::{Click, KEYS};
use crate::model::{format_duration, Model};
use crate::text::via;
use ultar_bus::messages::call::{CallInfo, Phase, Way};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Badge, Button, ButtonVariant, Card, Dialog, Icon, Label, Text, Tone};

fn avatar(b: &mut Builder<'_>, parent: NodeId) -> Result<NodeId, UiError> {
    let side = Space::S48.points();
    let flex = Flex::column()
        .main(MainAlign::Center)
        .cross(CrossAlign::Center);
    let card = b.add(
        parent,
        Card::new().raised().flex(flex),
        Sizing::width(side).with_height(side),
    )?;
    b.add(card, Icon::new(IconId::User), Sizing::AUTO)?;
    Ok(card)
}

fn badges(
    b: &mut Builder<'_>,
    parent: NodeId,
    call: &CallInfo,
    quality: bool,
) -> Result<(), UiError> {
    let row = b.flex(parent, Flex::row().gap(Space::S8).wrap(), Sizing::AUTO)?;
    if !quality {
        b.add(
            row,
            Badge::new(via(call.way)).tone(Tone::Info).with_icon(),
            Sizing::AUTO,
        )?;
    }
    if call.encrypted {
        let e2e = Badge::new(Label::key("phone.e2e"))
            .tone(Tone::Success)
            .with_icon();
        b.add(row, e2e, Sizing::AUTO)?;
    }
    if call.key_changed {
        // the other number's device key is new: compare the safety number before trusting
        let changed = Badge::new(Label::key("phone.key_changed"))
            .tone(Tone::Warning)
            .with_icon();
        b.add(row, changed, Sizing::AUTO)?;
    }
    if let Some(n) = &call.safety {
        let safety = Badge::new(Label::key_with("phone.safety", &[("n", n)])).tone(Tone::Neutral);
        b.add(row, safety, Sizing::AUTO)?;
    }
    if quality && call.phase == Phase::Active {
        let q = Badge::new(Label::key("phone.quality.good"))
            .tone(Tone::Success)
            .with_icon();
        b.add(row, q, Sizing::AUTO)?;
    }
    Ok(())
}

/// The ringing call.
pub(crate) fn incoming(
    b: &mut Builder<'_>,
    body: NodeId,
    model: &Model,
    id: u32,
) -> Result<(), UiError> {
    let Some(call) = model.call(id) else {
        return Ok(());
    };
    let flex = Flex::column()
        .padding(Padding::all(Space::S24))
        .gap(Space::S24)
        .main(MainAlign::Center)
        .cross(CrossAlign::Center);
    let main = b.flex(body, flex, Sizing::grow(1))?;
    b.name(
        main,
        Label::key_with("phone.incoming.name", &[("name", Model::who(call))]),
    )?;
    avatar(b, main)?;
    let names = b.flex(main, Flex::column().cross(CrossAlign::Center), Sizing::AUTO)?;
    let kind = if call.media.video_call {
        "phone.incoming.video"
    } else {
        "phone.incoming.voice"
    };
    b.add(
        names,
        Text::new(Label::key(kind)).color(ColorRole::TextMuted),
        Sizing::AUTO,
    )?;
    let size = if b.compact() {
        TypeSize::S24
    } else {
        TypeSize::S32
    };
    b.add(
        names,
        heading(Label::data(Model::who(call)), size, Weight::Bold),
        Sizing::AUTO,
    )?;
    if call.name.is_some() {
        b.add(
            names,
            mono(Label::data(&call.number), TypeSize::S16),
            Sizing::AUTO,
        )?;
    }
    badges(b, main, call, false)?;
    answer_buttons(b, main, id)?;
    b.add(main, small(Label::key("phone.incoming.note")), Sizing::AUTO)?;
    Ok(())
}

/// Decline, decline with message, answer (Answer takes the keyboard focus).
fn answer_buttons(b: &mut Builder<'_>, main: NodeId, id: u32) -> Result<(), UiError> {
    let row = b.flex(
        main,
        Flex::row().gap(Space::S16).wrap().main(MainAlign::Center),
        Sizing::AUTO,
    )?;
    let decline = Button::new(Label::key("phone.decline"))
        .icon(IconId::Hangup)
        .variant(ButtonVariant::Danger);
    b.click(row, decline, Sizing::AUTO, Click::Decline(id))?;
    let msg = Button::new(Label::key("phone.decline.msg")).icon(IconId::Message);
    b.click(row, msg, Sizing::AUTO, Click::DeclineMessage(id))?;
    let accept = Button::new(Label::key("phone.accept"))
        .icon(IconId::Phone)
        .variant(ButtonVariant::Primary);
    let accept = b.click(row, accept, Sizing::AUTO, Click::Accept(id))?;
    // Answer takes the keyboard focus; before the first layout it is not focusable yet and the
    // keyboard then starts at the first control, which is just as good.
    let _ = b.ui.focus(accept);
    Ok(())
}

/// The call in progress.
pub(crate) fn in_call(
    b: &mut Builder<'_>,
    body: NodeId,
    model: &Model,
    id: u32,
) -> Result<(), UiError> {
    let Some(call) = model.call(id) else {
        return Ok(());
    };
    let main = section(b, body, Space::S16)?;
    let key = if call.media.video_call {
        "phone.incall.video"
    } else {
        "phone.incall.voice"
    };
    b.name(main, Label::key_with(key, &[("name", Model::who(call))]))?;
    info(b, main, model, call)?;
    if let Some(other) = model.other_call(id) {
        second_call(b, main, other)?;
    }
    picture(b, main, call)?;
    controls(b, main, model, call)?;
    if model.keypad {
        keypad(b, main, model)?;
    }
    Ok(())
}

fn info(b: &mut Builder<'_>, main: NodeId, model: &Model, call: &CallInfo) -> Result<(), UiError> {
    let row = b.flex(
        main,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
        Sizing::AUTO,
    )?;
    avatar(b, row)?;
    let texts = b.flex(row, Flex::column(), Sizing::grow(1))?;
    b.add(
        texts,
        heading(Label::data(Model::who(call)), TypeSize::S20, Weight::Bold),
        Sizing::AUTO,
    )?;
    let line = b.flex(texts, Flex::row().gap(Space::S8), Sizing::AUTO)?;
    let status = match call.phase {
        Phase::Dialing => Label::key("phone.phase.dialing"),
        Phase::Alerting => Label::key("phone.phase.alerting"),
        Phase::Held => Label::key("phone.phase.held"),
        _ => Label::data(&format_duration(model.duration_s(call))),
    };
    let timer = b.add(line, small(status), Sizing::AUTO)?;
    if call.phase == Phase::Active {
        b.built.timer = Some(timer);
    }
    b.add(line, small(Label::data("·")), Sizing::AUTO)?;
    b.add(line, small(via(call.way)), Sizing::AUTO)?;
    let holder = if b.compact() { main } else { row };
    badges(b, holder, call, true)
}

fn second_call(b: &mut Builder<'_>, main: NodeId, other: &CallInfo) -> Result<(), UiError> {
    let flex = Flex::row()
        .padding(Padding::symmetric(Space::S16, Space::S8))
        .gap(Space::S8)
        .cross(CrossAlign::Center)
        .wrap();
    let card = b.add(main, Card::new().raised().flex(flex), Sizing::AUTO)?;
    let who = Model::who(other);
    if other.phase.is_incoming() {
        let text = Text::new(Label::key_with("phone.waiting", &[("name", who)]));
        b.add(card, text, Sizing::grow(1))?;
        let hold = Button::new(Label::key("phone.waiting.hold"))
            .icon(IconId::Phone)
            .variant(ButtonVariant::Primary);
        b.click(card, hold, Sizing::AUTO, Click::Accept(other.id))?;
        let end = Button::new(Label::key("phone.waiting.end"));
        b.click(card, end, Sizing::AUTO, Click::AcceptEnd(other.id))?;
        let decline = Button::new(Label::key("phone.decline"))
            .icon(IconId::Hangup)
            .variant(ButtonVariant::Danger);
        b.click(card, decline, Sizing::AUTO, Click::Decline(other.id))?;
    } else {
        let text = Text::new(Label::key_with("phone.held", &[("name", who)]));
        b.add(card, text, Sizing::grow(1))?;
        let swap = Button::new(Label::key("phone.swap")).icon(IconId::Pause);
        b.click(card, swap, Sizing::AUTO, Click::Swap(other.id))?;
    }
    Ok(())
}

fn picture(b: &mut Builder<'_>, main: NodeId, call: &CallInfo) -> Result<(), UiError> {
    let flex = Flex::column()
        .gap(Space::S8)
        .main(MainAlign::Center)
        .cross(CrossAlign::Center)
        .padding(Padding::all(Space::S16));
    let area = b.add(main, Card::new().flex(flex), Sizing::grow(1))?;
    b.add(
        area,
        Icon::new(IconId::User).color(ColorRole::TextMuted),
        Sizing::AUTO,
    )?;
    let peer = Label::key_with("phone.video.peer", &[("name", Model::who(call))]);
    b.add(
        area,
        Text::new(peer).color(ColorRole::TextMuted),
        Sizing::AUTO,
    )?;
    if call.media.camera_on {
        let (w, h) = metrics::own_picture();
        let own = Flex::column()
            .main(MainAlign::Center)
            .cross(CrossAlign::Center);
        let own = b.add(
            area,
            Card::new().raised().flex(own),
            Sizing::width(w).with_height(h),
        )?;
        b.add(own, Icon::new(IconId::Video).small(), Sizing::AUTO)?;
        b.add(own, small(Label::key("phone.video.own")), Sizing::AUTO)?;
    }
    Ok(())
}

fn controls(
    b: &mut Builder<'_>,
    main: NodeId,
    model: &Model,
    call: &CallInfo,
) -> Result<(), UiError> {
    let flex = Flex::row().gap(Space::S8).main(MainAlign::Center).wrap();
    let bar = b.flex(main, flex, Sizing::AUTO)?;
    b.name(bar, Label::key("phone.controls"))?;
    // Boards: icon above label in g(26)-wide tiles; on/off controls are toggles.
    for (icon, key, click, enabled, toggled) in control_entries(model, call) {
        let variant = if matches!(click, Click::Hangup(_)) {
            ButtonVariant::Danger
        } else {
            ButtonVariant::Plain
        };
        let mut btn = Button::new(Label::key(key))
            .icon(icon)
            .variant(variant)
            .tile();
        if let Some(on) = toggled {
            btn = btn.toggled(on);
        }
        let node = b.click(bar, btn, Sizing::width(metrics::control_width()), click)?;
        b.ui.set_disabled(node, !enabled)?;
    }
    Ok(())
}

/// One control: icon, label key, click, enabled, toggle state.
type Control = (IconId, &'static str, Click, bool, Option<bool>);

/// The eight controls of the board.
fn control_entries(model: &Model, call: &CallInfo) -> [Control; 8] {
    let id = call.id;
    let active = call.phase == Phase::Active;
    let held = call.phase == Phase::Held;
    let ultar = call.way == Way::Ultar && active;
    let m = call.media;
    let audio = &model.state.audio;
    let speaker = audio.output != ultar_bus::messages::call::Output::Earpiece;
    let camera = ultar && m.camera_on && m.cameras > 1;
    let on = active || held;
    let outputs = audio.outputs.len() > 1;
    use Click as C;
    use IconId as I;
    [
        (I::Mic, "phone.mute", C::Mute, on, Some(audio.muted)),
        (
            I::Speaker,
            "phone.output",
            C::Output,
            outputs,
            Some(speaker),
        ),
        (I::Keypad, "phone.keypad", C::Keypad, active, None),
        (I::Pause, "phone.hold", C::Hold(id), on, Some(held)),
        (
            I::Video,
            "phone.video",
            C::Video(id),
            ultar,
            Some(m.camera_on),
        ),
        (I::CameraSwitch, "phone.camera", C::Camera(id), camera, None),
        (
            I::ScreenShare,
            "phone.share",
            C::Share(id),
            ultar,
            Some(m.sharing),
        ),
        (I::Hangup, "phone.hangup", C::Hangup(id), true, None),
    ]
}

fn keypad(b: &mut Builder<'_>, main: NodeId, model: &Model) -> Result<(), UiError> {
    let sheet = Dialog::sheet(Label::key("phone.keypad")).flex(Flex::column().gap(Space::S8));
    let sheet = b.add(main, sheet, Sizing::AUTO)?;
    if !model.tones.is_empty() {
        b.add(
            sheet,
            mono(Label::data(&model.tones), TypeSize::S20),
            Sizing::AUTO,
        )?;
    }
    for row_keys in KEYS.chunks(3) {
        let row = b.flex(sheet, Flex::row().gap(Space::S8), Sizing::AUTO)?;
        let ordered: Vec<char> = if b.ui.env().direction == ultar_ui::Direction::Rtl {
            row_keys.iter().rev().copied().collect()
        } else {
            row_keys.to_vec()
        };
        for key in ordered {
            let k = Button::new(Label::data(&key.to_string()));
            let sizing = Sizing::grow(1).with_height(metrics::key_height());
            b.click(row, k, sizing, Click::Digit(key))?;
        }
    }
    let close = Button::new(Label::key("phone.keypad.close"));
    let close = b.click(sheet, close, Sizing::AUTO, Click::CloseDialog)?;
    b.ui.set_cancel_action(sheet, Some(close))?;
    b.ui.open_dialog(sheet)?;
    b.built.dialog = Some(sheet);
    Ok(())
}
