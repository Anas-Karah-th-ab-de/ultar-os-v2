//! The other questions as phone sheets: permission, administrator password, Bluetooth
//! pairing and "keep these display settings?" – the same texts and answers as the desktop's
//! dialogs, under `mobile.dialog.*`.

use super::sheet::{heading, Sheet};
use super::{DialogClick, Dialogs};
use ultar_bus::messages::bluetooth::{PairConfirm, PairQuestion};
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::messages::sysdialog::{AskPermission, Authenticate};
use ultar_bus::{Text as BusText, TextArg};
use ultar_dialog_core::KEEP_SECONDS;
use ultar_i18n::Translator;
use ultar_ui::icon::IconId;
use ultar_ui::layout::Sizing;
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::TypeSize;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{ButtonVariant, Label, Progress, Text, TextField};

/// Longest Bluetooth device name shown (characters; data from the air).
const MAX_DEVICE: usize = 64;

/// A text of another service (key and arguments) as a label of this catalog.
pub fn label_of(text: &BusText, tr: &Translator) -> Label {
    let values: Vec<(String, String)> = text
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let v = match a {
                TextArg::Verbatim(s) => s.clone(),
                TextArg::Number(n) => n.to_string(),
                TextArg::Key(k) => tr.t(k).into_owned(),
            };
            (format!("arg{i}"), v)
        })
        .collect();
    let args: Vec<(&str, &str)> = values
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    Label::key_with(text.key.clone(), &args)
}

/// What the app wants and why: always allow, allow once, deny.
pub fn permission(b: &mut Sheet<'_>, _body: NodeId, p: &AskPermission) -> Result<(), UiError> {
    let tr = b.ui.translator().clone();
    let word = format!("mobile.dialog.permission.{}", p.permission.word());
    let what = tr.t(&word).into_owned();
    let title = Label::key_with(
        "mobile.dialog.permission.title",
        &[("app", &p.app_name), ("what", &what)],
    );
    let reason = p
        .reason
        .as_ref()
        .map_or(Label::key("mobile.dialog.permission.text"), |r| {
            label_of(r, &tr)
        });
    b.texts(title, reason)?;
    let always = "mobile.dialog.allow_always";
    b.button(
        always,
        ButtonVariant::Primary,
        None,
        DialogClick::AllowAlways,
    )?;
    let once = "mobile.dialog.allow_once";
    b.button(once, ButtonVariant::Plain, None, DialogClick::AllowOnce)?;
    b.yes_no(None, "mobile.dialog.deny")?;
    Ok(())
}

/// `sysdialog.bt_pair`: consent, the number to compare, or the number to type on the device.
pub fn bt_pair(b: &mut Sheet<'_>, body: NodeId, p: &PairConfirm) -> Result<(), UiError> {
    let name = p.device_name.chars().take(MAX_DEVICE).collect::<String>();
    let who = [("name", name.as_str())];
    let (title, number, yes, no) = match p.question {
        PairQuestion::Consent => (
            Label::key_with("mobile.dialog.bt.consent", &who),
            None,
            Some("mobile.dialog.bt.pair"),
            "mobile.dialog.cancel",
        ),
        PairQuestion::Compare(n) => (
            Label::key_with("mobile.dialog.bt.compare", &who),
            Some(format!("{n:06}")),
            Some("mobile.dialog.bt.yes"),
            "mobile.dialog.bt.no",
        ),
        PairQuestion::Show(n) => {
            let number = format!("{n:06}");
            let args = [("name", name.as_str()), ("number", number.as_str())];
            let title = Label::key_with("mobile.dialog.bt.show", &args);
            (title, Some(number), None, "mobile.dialog.cancel")
        }
    };
    match number {
        None => b.texts(title, Label::key("mobile.dialog.bt.consent.text"))?,
        Some(number) => {
            b.add(body, Text::new(title).spec(heading()), Sizing::AUTO)?;
            // The number large, in digits only (no letters to misread).
            let big = TextSpec::new(TypeSize::S32).weight(Weight::Bold);
            b.add(
                body,
                Text::new(Label::data(&number)).spec(big),
                Sizing::AUTO,
            )?;
        }
    }
    b.yes_no(yes.map(|k| (k, DialogClick::Accept)), no)?;
    Ok(())
}

/// The error under the password field.
fn auth_error(d: &Dialogs, now_ms: u64) -> Option<Label> {
    let waiting = d.wrong_until.map_or(0, |t| t.saturating_sub(now_ms));
    match (d.admin.is_none(), d.wrong_until) {
        (true, _) => Some(Label::key("mobile.dialog.auth.no_admin")),
        (false, Some(_)) if waiting > 0 => Some(Label::key_with(
            "mobile.dialog.auth.wait",
            &[("seconds", &waiting.div_ceil(1000).to_string())],
        )),
        (false, Some(_)) => Some(Label::key("mobile.dialog.auth.wrong")),
        _ => None,
    }
}

/// An administrator's password for an action of an app.
pub fn auth(
    b: &mut Sheet<'_>,
    body: NodeId,
    d: &Dialogs,
    a: &Authenticate,
    now_ms: u64,
) -> Result<(), UiError> {
    let tr = b.ui.translator().clone();
    let action = label_of(&a.action, &tr).resolve(&tr).into_owned();
    let args = [("app", a.app_name.as_str()), ("action", action.as_str())];
    let text = Label::key_with("mobile.dialog.auth.text", &args);
    b.texts(Label::key("mobile.dialog.auth.title"), text)?;
    let name = d
        .admin
        .as_ref()
        .map_or(String::new(), |u| u.display_name.clone());
    let label = Label::key_with("mobile.dialog.auth.password", &[("user", &name)]);
    let field = TextField::password(label).max_len(ultar_bus::messages::auth::MAX_SECRET);
    let field = b.add(body, field, Sizing::AUTO)?;
    b.built.password = Some(field);
    b.ui.set_error(field, auth_error(d, now_ms))?;
    let waiting = d.wrong_until.is_some_and(|t| t > now_ms);
    let blocked = d.checking || waiting || d.admin.is_none();
    b.ui.set_disabled(field, blocked)?;
    let confirm = "mobile.dialog.auth.confirm";
    let ok = b.button(
        confirm,
        ButtonVariant::Primary,
        None,
        DialogClick::Authenticate,
    )?;
    b.ui.set_disabled(ok, blocked)?;
    b.ui.set_loading(ok, d.checking)?;
    let cancel = b.yes_no(None, "mobile.dialog.cancel")?;
    b.built.focus = Some(if blocked { cancel } else { field });
    Ok(())
}

/// "Keep these display settings?" with the countdown: keep, revert, more time.
pub fn keep(
    b: &mut Sheet<'_>,
    body: NodeId,
    value: &SettingValue,
    remaining_ms: u64,
) -> Result<(), UiError> {
    let tr = b.ui.translator().clone();
    let mode = match value {
        SettingValue::Text(t) => format!("mobile.dialog.keep.mode.{t}"),
        _ => "mobile.dialog.keep.mode.other".to_owned(),
    };
    let mode = tr.t(&mode).into_owned();
    let text = Label::key_with("mobile.dialog.keep.text", &[("mode", &mode)]);
    b.texts(Label::key("mobile.dialog.keep.title"), text)?;
    let seconds = remaining_ms.div_ceil(1000).min(u64::from(KEEP_SECONDS) * 4);
    let timer = Label::key_with(
        "mobile.dialog.keep.timer",
        &[("seconds", &seconds.to_string())],
    );
    let spec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
    b.add(body, Text::new(timer).spec(spec), Sizing::AUTO)?;
    let share = (remaining_ms as f32 / (KEEP_SECONDS as f32 * 1000.0)).clamp(0.0, 1.0);
    let progress = Progress::new(Label::key("mobile.dialog.keep.left")).value(share);
    b.add(body, progress, Sizing::AUTO)?;
    let keep = "mobile.dialog.keep.keep";
    b.button(
        keep,
        ButtonVariant::Primary,
        Some(IconId::Check),
        DialogClick::Keep,
    )?;
    b.yes_no(None, "mobile.dialog.keep.revert")?;
    let more = "mobile.dialog.keep.more";
    b.button(more, ButtonVariant::Quiet, None, DialogClick::MoreTime)?;
    Ok(())
}
