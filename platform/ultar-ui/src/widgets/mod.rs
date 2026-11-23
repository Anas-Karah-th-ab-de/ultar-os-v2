//! The widgets every Ultar OS surface is built from (design spec §3 "Bausteine").
//!
//! A widget is plain data plus a `measure` and a `paint` function; interaction state lives in
//! [`crate::ui::Ui`] and reaches the widget as a [`Visual`].

mod badge;
mod button;
mod checkbox;
mod containers;
mod dialog;
mod info_bar;
mod label;
mod list;
mod menu;
mod progress;
mod radio;
mod scroll;
mod search_field;
mod select;
mod slider;
mod style;
mod switch;
mod tabs;
mod text_field;
mod tone;
mod tooltip;

pub use badge::Badge;
pub use button::{Button, ButtonVariant, IconButton};
pub use checkbox::Checkbox;
pub use containers::{Card, Icon, Row, Text};
pub use dialog::{Dialog, DialogKind};
pub use info_bar::{EmptyState, InfoBar};
pub use label::{Label, LabelArg};
pub use list::{List, ListItem};
pub use menu::{Menu, MenuItem, MAX_MENU_DEPTH, MAX_MENU_ITEMS};
pub use progress::Progress;
pub use radio::RadioGroup;
pub use scroll::Scroll;
pub use search_field::SearchField;
pub use select::{Select, MAX_VISIBLE_OPTIONS};
pub use slider::Slider;
pub use style::Visual;
pub use switch::Switch;
pub use tabs::Tabs;
pub use text_field::{TextDirection, TextField, DEFAULT_MAX_LEN};
pub use tone::Tone;

pub(crate) use containers::shadow_reach;
pub(crate) use menu::MenuPanel;
pub(crate) use select::PopupView;
pub(crate) use style::focus_outset;
pub(crate) use tooltip::{paint_tooltip, tooltip_rect};

use crate::geom::Size;
use crate::layout::Flex;
use crate::paint::{layout_text, PaintEnv, Resources, TextSpec};
use crate::tokens::InputMode;
use ultar_i18n::Translator;

/// What measuring needs: fonts, environment, catalog, input mode.
pub(crate) struct MeasureCx<'a> {
    pub(crate) res: &'a mut Resources,
    pub(crate) env: &'a PaintEnv,
    pub(crate) tr: &'a Translator,
    pub(crate) mode: InputMode,
}

impl MeasureCx<'_> {
    /// Size of a label in logical points; `max` limits the width for wrap and ellipsis.
    pub(crate) fn text_size(&mut self, label: &Label, spec: TextSpec, max: Option<f32>) -> Size {
        let text = label.resolve(self.tr);
        let layout = layout_text(self.res, self.env, &text, spec, max);
        Size::new(
            (layout.width / self.env.scale).ceil(),
            layout.height / self.env.scale,
        )
    }
}

/// What painting needs besides the painter.
pub(crate) struct PaintCx<'a> {
    pub(crate) tr: &'a Translator,
    pub(crate) mode: InputMode,
    /// Rotation of progress rings, 0..1.
    pub(crate) spin: f32,
}

/// Any node of a UI tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    /// A row or column of children.
    Flex(Flex),
    /// A text block.
    Text(Text),
    /// A stroke icon.
    Icon(Icon),
    /// A push button.
    Button(Button),
    /// A button showing only an icon.
    IconButton(IconButton),
    /// An on/off switch.
    Switch(Switch),
    /// A choice of one option from a list.
    Select(Select),
    /// A value on a range.
    Slider(Slider),
    /// A single-line text input (also the search field).
    TextField(TextField),
    /// A settings row: label, sublabel and one control child.
    Row(Row),
    /// A card grouping children.
    Card(Card),
    /// A status word in a pill.
    Badge(Badge),
    /// A progress bar.
    Progress(Progress),
    /// A yes/no choice with its label.
    Checkbox(Checkbox),
    /// One of a few visible options.
    RadioGroup(RadioGroup),
    /// A tab bar.
    Tabs(Tabs),
    /// A message of one tone with an optional action child.
    InfoBar(InfoBar),
    /// What an empty view shows, with an optional action child.
    EmptyState(EmptyState),
    /// A column of list items.
    List(List),
    /// A row of a list with an optional trailing control child.
    ListItem(ListItem),
    /// A vertical scroll container.
    Scroll(Scroll),
    /// A dialog or sheet above everything else.
    Dialog(Dialog),
}

macro_rules! widget_from {
    ($( $ty:ident ),*) => {
        $( impl From<$ty> for Widget {
            fn from(value: $ty) -> Widget {
                Widget::$ty(value)
            }
        } )*
    };
}

widget_from!(
    Flex, Text, Icon, Button, IconButton, Switch, Select, Slider, TextField, Row, Card, Badge,
    Progress, Checkbox, RadioGroup, Tabs, InfoBar, EmptyState, List, ListItem, Scroll, Dialog
);

impl From<SearchField> for Widget {
    fn from(value: SearchField) -> Widget {
        Widget::TextField(value.into_field())
    }
}

impl Widget {
    /// `true` for widgets the user operates (they take focus, hover and presses). A scroll
    /// container is operable only while its content overflows; the tree decides that.
    pub fn is_interactive(&self) -> bool {
        matches!(
            self,
            Widget::Button(_)
                | Widget::IconButton(_)
                | Widget::Switch(_)
                | Widget::Select(_)
                | Widget::Slider(_)
                | Widget::TextField(_)
                | Widget::Checkbox(_)
                | Widget::RadioGroup(_)
                | Widget::Tabs(_)
                | Widget::ListItem(_)
        )
    }

    /// `true` for widgets that can show a loading state.
    pub fn can_load(&self) -> bool {
        matches!(
            self,
            Widget::Button(_) | Widget::IconButton(_) | Widget::Switch(_) | Widget::Select(_)
        )
    }

    /// `true` for widgets that can show an error with a message.
    pub fn can_fail(&self) -> bool {
        matches!(self, Widget::Select(_) | Widget::TextField(_))
    }

    /// The layout container of this widget, if it lays out children as a flex.
    pub fn flex(&self) -> Option<&Flex> {
        match self {
            Widget::Flex(flex) => Some(flex),
            Widget::Card(card) => Some(&card.flex),
            Widget::List(list) => Some(&list.flex),
            Widget::Scroll(scroll) => Some(&scroll.flex),
            Widget::Dialog(dialog) => Some(&dialog.flex),
            _ => None,
        }
    }

    /// `true` for widgets that hold exactly one child, placed by the widget itself (the
    /// control of a settings row, the action of an info bar or empty state, the trailing
    /// control of a list item).
    pub fn hosts_one_child(&self) -> bool {
        matches!(
            self,
            Widget::Row(_) | Widget::InfoBar(_) | Widget::EmptyState(_) | Widget::ListItem(_)
        )
    }
}
