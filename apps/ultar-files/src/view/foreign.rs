//! The install sheet for apps from other systems (`crate::foreign`): what the file says
//! about itself (always marked as not checked), what the app may use (everything off at
//! first), the job's steps and its end.

use super::Built;
use crate::action::Target;
use crate::foreign::{self, Phase, Sheet};
use ultar_bus::messages::foreign::{Inspected, NetworkChoice};
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Checkbox, InfoBar, Label, Progress, Row, Scroll, Text, Tone};

/// The sheet's title.
pub fn title(sheet: &Sheet) -> Label {
    let key = match sheet.phase {
        Phase::Done(_) => "files.foreign.title.done",
        Phase::Problem(_) => "files.foreign.title.problem",
        _ => "files.foreign.title",
    };
    Label::key_with(key, &[("name", &sheet.file)])
}

/// Keys of the sheet's buttons: (close or cancel, confirm).
pub fn buttons(sheet: &Sheet) -> (&'static str, Option<&'static str>) {
    match sheet.phase {
        Phase::Ready(_) => ("files.cancel", Some("files.foreign.install")),
        Phase::Done(ref apps) if !apps.is_empty() => ("files.close", Some("files.foreign.open")),
        Phase::Reading => ("files.cancel", None),
        _ => ("files.close", None),
    }
}

/// The content of the sheet.
pub fn body(ui: &mut Ui, b: &mut Built, dialog: NodeId, sheet: &Sheet) -> Result<(), UiError> {
    match &sheet.phase {
        Phase::Reading => muted(ui, dialog, Label::key("files.foreign.reading")),
        Phase::Ready(i) => ready(ui, b, dialog, sheet, i),
        Phase::Starting => muted(ui, dialog, Label::key("files.foreign.starting")),
        Phase::Running { step, percent, .. } => {
            let name = Label::key(foreign::step_key(*step));
            ui.add(dialog, Progress::new(name).value(f32::from(*percent) / 100.0))?;
            muted(ui, dialog, Label::key("files.foreign.running_hint"))
        }
        Phase::Done(_) => muted(
            ui,
            dialog,
            Label::key_with("files.foreign.done", &[("name", &sheet.file)]),
        ),
        Phase::Problem(key) => muted(ui, dialog, Label::key(*key)),
    }
}

fn muted(ui: &mut Ui, parent: NodeId, label: Label) -> Result<(), UiError> {
    ui.add(parent, Text::new(label).color(ColorRole::TextMuted))
        .map(|_| ())
}

fn ready(
    ui: &mut Ui,
    b: &mut Built,
    dialog: NodeId,
    sheet: &Sheet,
    i: &Inspected,
) -> Result<(), UiError> {
    let scroll = ui.add_sized(
        dialog,
        Scroll::new()
            .flex(Flex::column().gap(Space::S8))
            .name(Label::key("files.foreign.details")),
        Sizing::grow(1),
    )?;
    let warning = InfoBar::new(Tone::Warning, Label::key("files.foreign.unchecked"))
        .message(Label::key("files.foreign.unchecked_hint"));
    ui.add(scroll, warning)?;
    facts(ui, scroll, i)?;
    muted(ui, scroll, Label::key("files.foreign.may_use"))?;
    let net = sheet.choices.network == NetworkChoice::Internet;
    let internet = Checkbox::new(Label::key("files.foreign.internet"), net)
        .sublabel(Label::key("files.foreign.internet_hint"))
        .wide();
    let id = ui.add(scroll, internet)?;
    b.mark(id, Target::ForeignInternet);
    if net {
        let lan = Checkbox::new(Label::key("files.foreign.lan"), sheet.choices.lan)
            .sublabel(Label::key("files.foreign.lan_hint"))
            .wide();
        let id = ui.add(scroll, lan)?;
        b.mark(id, Target::ForeignLan);
    }
    muted(ui, scroll, Label::key("files.foreign.folders"))?;
    for (word, key) in foreign::FOLDERS {
        let on = sheet.choices.folders.iter().any(|f| f == word);
        let id = ui.add(scroll, Checkbox::new(Label::key(key), on).wide())?;
        b.mark(id, Target::ForeignFolder(word.to_owned()));
    }
    Ok(())
}

/// What the file says about itself (its own words, as data).
fn facts(ui: &mut Ui, parent: NodeId, i: &Inspected) -> Result<(), UiError> {
    let size = ultar_i18n::units::format_size(i.size_bytes, ui.translator());
    let publisher = if i.publisher.is_empty() {
        Label::key("files.foreign.no_publisher")
    } else {
        Label::key_with("files.foreign.publisher_said", &[("name", &i.publisher)])
    };
    let rows = [
        ("files.foreign.kind", Label::key(foreign::kind_key(i.source))),
        ("files.foreign.name", Label::data(&i.name)),
        ("files.foreign.version", Label::data(&i.version)),
        ("files.prop.size", Label::data(&size)),
        ("files.foreign.publisher", publisher),
    ];
    for (key, value) in rows {
        ui.add(parent, Row::new(Label::key(key)).sublabel(value))?;
    }
    Ok(())
}
