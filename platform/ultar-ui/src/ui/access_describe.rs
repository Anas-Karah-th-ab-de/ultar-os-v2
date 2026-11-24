//! Role, name and value of each widget for the accessibility tree.

use super::Role;
use crate::widgets::{CardTone, DialogKind, Label, Tone, Widget};

fn number(value: f32) -> Label {
    let text = if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    Label::data(&text)
}

fn percent(share: f32) -> Label {
    Label::data(&format!("{:.0}%", (share * 100.0).clamp(0.0, 100.0)))
}

/// Role, name and value of a widget; `None` for pure layout and unnamed icons.
pub(super) fn describe_widget(widget: &Widget) -> Option<(Role, Option<Label>, Option<Label>)> {
    let named = |label: &Label| Some(label.clone());
    Some(match widget {
        Widget::Flex(_) => return None,
        Widget::Icon(w) => (Role::Image, Some(w.name.clone()?), None),
        Widget::Card(c) if c.tone == CardTone::Danger => (Role::Alert, None, None),
        Widget::Card(_) => (Role::Group, None, None),
        Widget::Text(w) => (Role::Text, named(&w.label), None),
        Widget::Row(w) => (Role::Row, named(&w.label), w.sublabel.clone()),
        Widget::Button(w) => (Role::Button, named(&w.label), None),
        Widget::IconButton(w) => (Role::Button, named(&w.name), None),
        Widget::Brick(w) => return describe_brick(w),
        Widget::DayGrid(w) => (Role::Table, named(&w.name), w.selected_name()),
        Widget::Switch(w) => (Role::Switch, named(&w.name), None),
        Widget::Select(w) => (
            Role::Select,
            named(&w.name),
            w.options.get(w.selected).cloned(),
        ),
        Widget::Slider(w) => (Role::Slider, named(&w.name), Some(number(w.value))),
        Widget::TextField(w) if w.is_password() => (Role::PasswordField, named(&w.name), None),
        Widget::TextField(w) => {
            let role = if w.is_search() {
                Role::SearchField
            } else {
                Role::TextField
            };
            (role, named(&w.name), Some(Label::data(w.content())))
        }
        Widget::Badge(w) => (Role::Badge, named(&w.label), None),
        Widget::Progress(w) => (Role::Progress, named(&w.name), w.value.map(percent)),
        Widget::Checkbox(w) => (Role::Checkbox, named(&w.label), None),
        Widget::RadioGroup(_) | Widget::Tabs(_) | Widget::Arrangement(_) => {
            return describe_choice(widget)
        }
        Widget::InfoBar(w) => {
            let role = match w.tone {
                Tone::Warning | Tone::Danger => Role::Alert,
                _ => Role::Status,
            };
            (role, named(&w.title), w.message.clone())
        }
        Widget::EmptyState(w) => (Role::Status, named(&w.title), w.message.clone()),
        Widget::List(w) if w.navigation => (Role::Navigation, w.name.clone(), None),
        Widget::List(w) => (Role::List, w.name.clone(), None),
        Widget::ListItem(w) => (Role::ListItem, named(&w.label), w.sublabel.clone()),
        Widget::Scroll(w) => (Role::ScrollArea, w.name.clone(), None),
        Widget::Dialog(w) if w.alert && w.kind == DialogKind::Dialog => {
            (Role::AlertDialog, named(&w.title), None)
        }
        Widget::Dialog(w) => (Role::Dialog, named(&w.title), None),
        Widget::Table(w) => (Role::Table, named(&w.name), None),
        Widget::Tree(w) => (Role::Tree, named(&w.name), None),
        Widget::HeaderBar(w) => (Role::Banner, named(&w.title), None),
        Widget::QrCode(w) => (Role::Image, named(&w.name), None),
    })
}

/// A shell brick: a rule is not announced, pictures are images, quick tiles switches.
fn describe_brick(w: &crate::widgets::Brick) -> Option<(Role, Option<Label>, Option<Label>)> {
    let name = Some(w.accessible_name().clone());
    Some(match w.kind {
        crate::widgets::BrickKind::Rule => return None,
        _ if w.is_static() => (Role::Image, name, None),
        _ if w.is_toggle() => (Role::Switch, name, None),
        _ => (Role::Button, name, None),
    })
}

/// Role, name and chosen value of a widget that offers one choice among parts.
fn describe_choice(widget: &Widget) -> Option<(Role, Option<Label>, Option<Label>)> {
    Some(match widget {
        Widget::RadioGroup(w) => {
            let chosen = w.selected.and_then(|i| w.options.get(i)).cloned();
            (Role::RadioGroup, Some(w.name.clone()), chosen)
        }
        Widget::Tabs(w) => (
            Role::TabList,
            Some(w.name.clone()),
            w.tabs.get(w.selected).cloned(),
        ),
        Widget::Arrangement(w) => {
            let chosen = w
                .selected
                .and_then(|i| w.items.get(i))
                .map(|i| i.name.clone());
            (Role::RadioGroup, Some(w.name.clone()), chosen)
        }
        _ => return None,
    })
}

/// The tone of a badge or info bar as a spoken description.
pub(super) fn tone_of(widget: &Widget) -> Option<Label> {
    let tone = match widget {
        Widget::Badge(badge) => badge.tone,
        Widget::InfoBar(bar) => bar.tone,
        _ => return None,
    };
    tone.name_key().map(Label::key)
}
