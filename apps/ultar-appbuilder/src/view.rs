//! The window, from `ultar-ui` widgets only: project field, the three actions, the status
//! line, the findings and the developer key.

use crate::state::{Phase, State, Target};
use std::collections::HashMap;
use ultar_ui::layout::{Flex, Padding};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Label, Text, TextField};

/// What each operable node means.
#[derive(Debug, Default)]
pub struct Built {
    /// Node → target.
    pub targets: HashMap<NodeId, Target>,
    /// The project field.
    pub project: Option<NodeId>,
}

fn status_label(s: &State) -> Label {
    match &s.phase {
        Phase::Idle => Label::key("appbuilder.state.idle"),
        Phase::Busy => Label::key("appbuilder.state.busy"),
        Phase::Checked { id, version } => Label::key_with(
            "appbuilder.state.checked",
            &[("id", id), ("version", version)],
        ),
        Phase::Built(path) => Label::key_with(
            "appbuilder.state.built",
            &[("path", &path.display().to_string())],
        ),
        Phase::Installed(id) => Label::key_with("appbuilder.state.installed", &[("id", id)]),
        Phase::Failed(e) => Label::key(e.message_key()),
    }
}

/// Builds the whole window for `s`.
pub fn build(ui: &mut Ui, s: &State) -> Result<Built, UiError> {
    ui.clear();
    ui.set_root_flex(
        Flex::column()
            .gap(Space::S16)
            .padding(Padding::all(Space::S24)),
    );
    let root = ui.root();
    let mut b = Built::default();
    let field = TextField::new(Label::key("appbuilder.project"))
        .text(&s.project)
        .labelled();
    let field = ui.add(root, field)?;
    b.targets.insert(field, Target::Project);
    b.project = Some(field);

    let bar = ui.add(root, Card::new().flex(Flex::row().gap(Space::S8)))?;
    for (key, target, variant) in [
        ("appbuilder.check", Target::Check, ButtonVariant::Plain),
        ("appbuilder.build", Target::Build, ButtonVariant::Primary),
        ("appbuilder.install", Target::Install, ButtonVariant::Plain),
    ] {
        let id = ui.add(bar, Button::new(Label::key(key)).variant(variant))?;
        let disabled = s.busy() || (target == Target::Install && s.package.is_none());
        ui.set_disabled(id, disabled)?;
        b.targets.insert(id, target);
    }

    let tone = if matches!(s.phase, Phase::Failed(_)) {
        ColorRole::Danger
    } else {
        ColorRole::Text
    };
    ui.add(root, Text::new(status_label(s)).color(tone))?;
    for issue in &s.issues {
        let label = match issue {
            crate::Issue::MissingSection(section) => {
                Label::key_with(issue.message_key(), &[("section", section)])
            }
            _ => Label::key(issue.message_key()),
        };
        ui.add(root, Text::new(label).color(ColorRole::TextMuted))?;
    }
    key_card(ui, root, s)?;
    if let Some(project) = b.project {
        ui.focus(project)?;
    }
    Ok(b)
}

/// The developer key card (after the first build).
fn key_card(ui: &mut Ui, root: NodeId, s: &State) -> Result<(), UiError> {
    let Some(key) = &s.key else {
        return Ok(());
    };
    {
        let card = ui.add(root, Card::padded().flex(Flex::column().gap(Space::S8)))?;
        ui.add(card, Text::new(Label::key("appbuilder.key.title")))?;
        ui.add(
            card,
            Text::new(Label::key_with("appbuilder.key.id", &[("id", &key.id)])),
        )?;
        ui.add(
            card,
            Text::new(Label::key_with(
                "appbuilder.key.public",
                &[("key", &key.public_hex)],
            ))
            .color(ColorRole::TextMuted),
        )?;
        let file = publisher_file(s);
        ui.add(
            card,
            Text::new(Label::key_with("appbuilder.key.trust", &[("file", &file)]))
                .color(ColorRole::TextMuted),
        )?;
    }
    Ok(())
}

/// `<publisher>.ed25519` from the built package's id (`publisher.app-version.uap`).
fn publisher_file(s: &State) -> String {
    let publisher = s
        .package
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(|n| n.split('.').next())
        .unwrap_or("developer");
    format!("{publisher}.ed25519")
}
