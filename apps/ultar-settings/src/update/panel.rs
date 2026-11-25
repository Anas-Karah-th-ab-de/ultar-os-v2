//! Rows of a generic `settingsd` page: a switch, a choice or a slider stores its value at
//! once; a text field (texts, lists, numbers with many steps) when Enter is pressed.

use super::{load_panels, set, Ctl, Outcome};
use crate::areas::PageKind;
use crate::view::panel::LIST_SEPARATOR;
use ultar_bus::messages::settings::{ItemKind, SettingValue};
use ultar_ui::event::ActionKind;
use ultar_ui::ui::NodeId;
use ultar_ui::widgets::Tone;

/// Row `index` of the open panel reported `kind`.
pub(super) fn item(c: &mut Ctl<'_>, index: usize, node: NodeId, kind: ActionKind) -> Outcome {
    let PageKind::Panel(id) = c.state.page() else {
        return Outcome::default();
    };
    let Some(item) = c.state.panel(id).and_then(|p| p.items.get(index)).cloned() else {
        return Outcome::default();
    };
    let typed = || c.ui.text(node).map(str::to_owned).unwrap_or_default();
    let value = match (&item.kind, kind) {
        (ItemKind::Toggle, ActionKind::Toggled(on)) => SettingValue::Flag(on),
        (ItemKind::Choice { choices }, ActionKind::Selected(i)) => match choices.get(i) {
            Some(choice) => SettingValue::Text(choice.id.clone()),
            None => return Outcome::default(),
        },
        // a default app chosen from the launcher's candidates (the row is a Select)
        (ItemKind::Text { .. }, ActionKind::Selected(i))
            if item.key.starts_with("apps.default.") =>
        {
            let current = match &item.value {
                SettingValue::Text(t) => t.clone(),
                _ => String::new(),
            };
            let options = crate::view::panel::default_options(&c.state.more, &item.key, &current);
            match options.get(i) {
                Some(app) => SettingValue::Text(app.clone()),
                None => return Outcome::default(),
            }
        }
        (ItemKind::Integer { .. }, ActionKind::ValueChanged(v)) => {
            SettingValue::Integer(v.round() as i64)
        }
        (ItemKind::Integer { .. }, ActionKind::Submitted) => match typed().trim().parse() {
            Ok(n) => SettingValue::Integer(n),
            Err(_) => return Outcome::toast(Tone::Danger, "app.settings.panel.bad_number"),
        },
        (ItemKind::Text { .. }, ActionKind::Submitted) => SettingValue::Text(typed()),
        (ItemKind::List { .. }, ActionKind::Submitted) => SettingValue::List(list_of(&typed())),
        _ => return Outcome::default(),
    };
    let out = set(c, &item.key, value);
    if out.toast.is_none() {
        load_panels(c.state, c.ports);
    }
    out
}

/// The entries of a list typed as "a, b, c" (empty entries dropped).
pub fn list_of(text: &str) -> Vec<String> {
    text.split(LIST_SEPARATOR)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_split_at_commas_and_trimmed() {
        assert_eq!(list_of(" /home/a , ,/srv/b"), ["/home/a", "/srv/b"]);
        assert!(list_of("  ").is_empty());
    }
}
