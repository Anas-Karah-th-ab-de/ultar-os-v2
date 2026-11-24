//! The dialog brick of the boards (`gen.py` `dialog`: width `g(120)`, padding 24, gap 16,
//! title 16/600, text muted, actions at the end) inside the compositor's trusted frame:
//! permission, administrator password, "keep these display settings?" with its 15 s.

use super::{Dialogs, Question, KEEP_SECONDS};
use crate::view::calendar::label_of;
use crate::view::{new_ui, Look, Target, View};
use ultar_bus::messages::bluetooth::{PairConfirm, PairQuestion};
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::messages::sysdialog::AskPermission;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::layout::{Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Label, Progress, Text, TextField};

/// Width of a system dialog (`g(120)`).
pub const WIDTH: f32 = 120.0 * Space::S4.points();

/// Title of the trusted frame: the app that asks (data), or the system for "keep?".
pub fn frame_title(d: &Dialogs, tr: &Translator) -> String {
    match d.current().map(|e| &e.question) {
        Some(Question::Permission(p)) => p.app_name.clone(),
        Some(Question::Authenticate(a)) => a.app_name.clone(),
        Some(Question::Keep { .. } | Question::BtPair(_) | Question::Handoff(_)) | None => {
            tr.t("dialog.system").into_owned()
        }
    }
}

pub(super) fn texts(view: &mut View, title: Label, text: Label) -> Result<(), UiError> {
    let root = view.ui.root();
    let heading = TextSpec::new(TypeSize::S16).weight(Weight::Semibold);
    view.ui.add(root, Text::new(title).spec(heading))?;
    view.ui
        .add(root, Text::new(text).color(ColorRole::TextMuted))?;
    Ok(())
}

pub(super) fn button(
    view: &mut View,
    row: NodeId,
    key: &str,
    variant: ButtonVariant,
    target: Target,
) -> Result<NodeId, UiError> {
    let id = view
        .ui
        .add(row, Button::new(Label::key(key)).variant(variant))?;
    Ok(view.mark(id, target))
}

/// The permission question: what the app wants and why, deny / allow once / allow always.
fn permission(
    view: &mut View,
    root: NodeId,
    p: &AskPermission,
    tr: &Translator,
) -> Result<NodeId, UiError> {
    let what = tr
        .t(&format!("dialog.permission.{}", p.permission.word()))
        .into_owned();
    let title = Label::key_with(
        "dialog.permission.title",
        &[("app", &p.app_name), ("what", &what)],
    );
    let reason = p
        .reason
        .as_ref()
        .map_or(Label::key("dialog.permission.text"), |r| label_of(r, tr));
    texts(view, title, reason)?;
    let row = view
        .ui
        .add(root, Flex::row().gap(Space::S8).main(MainAlign::End))?;
    let deny = button(view, row, "dialog.deny", ButtonVariant::Plain, Target::Deny)?;
    button(
        view,
        row,
        "dialog.allow_once",
        ButtonVariant::Plain,
        Target::AllowOnce,
    )?;
    button(
        view,
        row,
        "dialog.allow_always",
        ButtonVariant::Primary,
        Target::AllowAlways,
    )?;
    Ok(deny)
}

/// `sysdialog.bt_pair` (docs/integration/system-services.md §3): consent, the number to
/// compare (six digits), or the number to type on the device; the device's name is data from
/// the air, shown as plain text.
fn bt_pair(view: &mut View, root: NodeId, p: &PairConfirm) -> Result<NodeId, UiError> {
    let name = p.device_name.chars().take(64).collect::<String>();
    let args = |n: u32| [("name", name.clone()), ("number", format!("{n:06}"))];
    let (title, text, yes, no) = match p.question {
        PairQuestion::Consent => (
            Label::key_with("dialog.bt.consent", &[("name", &name)]),
            Label::key("dialog.bt.consent.text"),
            Some("dialog.bt.pair"),
            "dialog.cancel",
        ),
        PairQuestion::Compare(n) => {
            let a = args(n);
            (
                Label::key_with("dialog.bt.compare", &[("name", &a[0].1)]),
                Label::data(&a[1].1),
                Some("dialog.bt.yes"),
                "dialog.bt.no",
            )
        }
        PairQuestion::Show(n) => {
            let a = args(n);
            (
                Label::key_with("dialog.bt.show", &[("name", &a[0].1), ("number", &a[1].1)]),
                Label::data(&a[1].1),
                None,
                "dialog.cancel",
            )
        }
    };
    let root_title = TextSpec::new(TypeSize::S16).weight(Weight::Semibold);
    view.ui.add(root, Text::new(title).spec(root_title))?;
    if matches!(p.question, PairQuestion::Consent) {
        view.ui
            .add(root, Text::new(text).color(ColorRole::TextMuted))?;
    } else {
        // The number large, in digits only (no letters to misread).
        let number = TextSpec::new(TypeSize::S24).weight(Weight::Bold);
        view.ui.add(root, Text::new(text).spec(number))?;
    }
    let row = view
        .ui
        .add(root, Flex::row().gap(Space::S8).main(MainAlign::End))?;
    let cancel = button(view, row, no, ButtonVariant::Plain, Target::Deny)?;
    match yes {
        Some(key) => button(view, row, key, ButtonVariant::Primary, Target::AllowOnce),
        None => Ok(cancel),
    }
}

/// The tree of the current question (`None` when nothing is asked); `now_ms` for the
/// countdown and the wait after a wrong password.
pub fn build(
    d: &Dialogs,
    now_ms: u64,
    look: Look,
    tr: &Translator,
) -> Result<Option<View>, UiError> {
    let Some(entry) = d.current() else {
        return Ok(None);
    };
    let mut ui = new_ui(look, Size::new(WIDTH, 600.0), tr)?;
    ui.set_background(ColorRole::Raised);
    ui.set_root_flex(
        Flex::column()
            .padding(Padding::all(Space::S24))
            .gap(Space::S16),
    );
    let mut view = View::new(ui);
    let root = view.ui.root();
    let focus = match &entry.question {
        Question::Permission(p) => permission(&mut view, root, p, tr)?,
        Question::BtPair(p) => bt_pair(&mut view, root, p)?,
        Question::Handoff(p) => super::handoff::handoff(&mut view, root, p, tr)?,
        Question::Authenticate(a) => auth(&mut view, d, a, now_ms, tr)?,
        Question::Keep { value, .. } => keep(
            &mut view,
            value,
            entry.deadline_ms.saturating_sub(now_ms),
            tr,
        )?,
    };
    view.ui.focus(focus)?;
    let height = view.ui.content_size().h;
    view.ui.set_size(Size::new(WIDTH, height))?;
    Ok(Some(view))
}

fn auth(
    view: &mut View,
    d: &Dialogs,
    a: &ultar_bus::messages::sysdialog::Authenticate,
    now_ms: u64,
    tr: &Translator,
) -> Result<NodeId, UiError> {
    let root = view.ui.root();
    let action = label_of(&a.action, tr).resolve(tr).into_owned();
    let text = Label::key_with(
        "dialog.auth.text",
        &[("app", &a.app_name), ("action", &action)],
    );
    texts(view, Label::key("dialog.auth.title"), text)?;
    let name = d
        .admin
        .as_ref()
        .map_or(String::new(), |u| u.display_name.clone());
    let field = TextField::password(Label::key_with("dialog.auth.password", &[("user", &name)]))
        .max_len(ultar_bus::messages::auth::MAX_SECRET);
    let field = view.ui.add(root, field)?;
    view.mark(field, Target::Password);
    let waiting = d.wrong_until.map_or(0, |t| t.saturating_sub(now_ms));
    let error = match (d.admin.is_none(), d.wrong_until) {
        (true, _) => Some(Label::key("dialog.auth.no_admin")),
        (false, Some(_)) if waiting > 0 => Some(Label::key_with(
            "lock.error.wait",
            &[("seconds", &waiting.div_ceil(1000).to_string())],
        )),
        (false, Some(_)) => Some(Label::key("lock.error.wrong")),
        _ => None,
    };
    view.ui.set_error(field, error)?;
    let blocked = d.checking || waiting > 0 || d.admin.is_none();
    view.ui.set_disabled(field, blocked)?;
    let row = view
        .ui
        .add(root, Flex::row().gap(Space::S8).main(MainAlign::End))?;
    let cancel = button(
        view,
        row,
        "dialog.cancel",
        ButtonVariant::Plain,
        Target::Deny,
    )?;
    let ok = button(
        view,
        row,
        "dialog.auth.confirm",
        ButtonVariant::Primary,
        Target::Confirm,
    )?;
    view.ui.set_disabled(ok, blocked)?;
    view.ui.set_loading(ok, d.checking)?;
    Ok(if blocked { cancel } else { field })
}

fn keep(
    view: &mut View,
    value: &SettingValue,
    remaining_ms: u64,
    tr: &Translator,
) -> Result<NodeId, UiError> {
    let root = view.ui.root();
    let mode = match value {
        SettingValue::Text(t) => format!("dialog.keep.mode.{t}"),
        _ => "dialog.keep.mode.other".to_owned(),
    };
    let mode = tr.t(&mode).into_owned();
    let text = Label::key_with("dialog.keep.text", &[("mode", &mode)]);
    texts(view, Label::key("dialog.keep.title"), text)?;
    let seconds = remaining_ms.div_ceil(1000).min(u64::from(KEEP_SECONDS) * 4);
    let timer = Text::new(Label::key_with(
        "dialog.keep.timer",
        &[("seconds", &seconds.to_string())],
    ))
    .spec(TextSpec::new(TypeSize::S14).weight(Weight::Semibold));
    view.ui.add(root, timer)?;
    let share = (remaining_ms as f32 / (KEEP_SECONDS as f32 * 1000.0)).clamp(0.0, 1.0);
    view.ui.add_sized(
        root,
        Progress::new(Label::key("dialog.keep.left")).value(share),
        Sizing::AUTO,
    )?;
    let row = view
        .ui
        .add(root, Flex::row().gap(Space::S8).main(MainAlign::End))?;
    button(
        view,
        row,
        "dialog.keep.more",
        ButtonVariant::Quiet,
        Target::MoreTime,
    )?;
    let revert = button(
        view,
        row,
        "dialog.keep.revert",
        ButtonVariant::Plain,
        Target::Deny,
    )?;
    button(
        view,
        row,
        "dialog.keep.keep",
        ButtonVariant::Primary,
        Target::Confirm,
    )?;
    Ok(revert)
}
