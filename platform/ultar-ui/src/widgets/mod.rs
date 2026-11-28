//! The widgets every Ultar OS surface is built from (design spec §3 "Bausteine").
//!
//! A widget is plain data plus a `measure` and a `paint` function; interaction state lives in
//! [`crate::ui::Ui`] and reaches the widget as a [`Visual`].

mod arrangement;
mod badge;
mod brick;
mod brick_art;
mod brick_logo;
mod brick_measure;
mod brick_overview;
mod brick_phone;
mod brick_quick;
mod brick_tile;
mod button;
mod checkbox;
mod containers;
mod day_grid;
mod dialog;
mod header_bar;
mod info_bar;
mod label;
mod list;
mod list_tile;
mod menu;
mod picture;
mod progress;
mod qr;
mod radio;
mod scroll;
mod search_field;
mod select;
mod slider;
mod style;
mod switch;
mod table;
mod tabs;
mod text_field;
mod thumbnail;
mod toast;
mod tone;
mod tooltip;
mod tree;

pub use arrangement::{ArrangedItem, Arrangement, MAX_ARRANGEMENT_ITEMS};
pub use badge::Badge;
pub use brick::{Brick, BrickKind, Picture};
pub use brick_overview::{Thumbnail, MAX_MINIS, MINIATURE, STRIP, WORKSPACE_WIDTH};
pub use button::{Button, ButtonVariant, IconButton};
pub use checkbox::Checkbox;
pub use containers::{BarEdge, Card, CardTone, Icon, Row, Text};
pub use day_grid::{DayCell, DayGrid, MAX_DAY_CELLS, WEEK};
pub use dialog::{Dialog, DialogKind};
pub use header_bar::HeaderBar;
pub use info_bar::{EmptyState, InfoBar};
pub use label::{Label, LabelArg};
pub use list::{List, ListItem};
pub use list_tile::TILE_WIDTH;
pub use menu::{Menu, MenuItem, MAX_MENU_DEPTH, MAX_MENU_ITEMS};
pub use picture::PixelArea;
pub use progress::Progress;
pub use qr::{QrCode, QrCodeError, MAX_QR_BYTES};
pub use radio::{Pictogram, RadioGroup, RadioLayout};
pub use scroll::Scroll;
pub use search_field::SearchField;
pub use select::{Select, MAX_VISIBLE_OPTIONS};
pub use slider::{Slider, NO_VALUE};
pub use style::Visual;
pub use switch::Switch;
pub use table::{Table, TableCursor, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS};
pub use tabs::Tabs;
pub use text_field::{TextDirection, TextField, DEFAULT_MAX_LEN};
pub use thumbnail::{Preview, MAX_THUMBNAIL_EDGE};
pub use toast::{Toast, MAX_TOASTS};
pub use tone::Tone;
pub use tree::{Tree, TreeItem, MAX_TREE_DEPTH, MAX_TREE_ITEMS};

pub(crate) use containers::shadow_reach;
pub(crate) use menu::MenuPanel;
pub(crate) use select::PopupView;
pub(crate) use style::focus_outset;
pub(crate) use table::TablePart;
pub(crate) use toast::{ToastPart, ToastView};
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
    /// The exact advance width of a one-line label in points (not rounded up): for rows of
    /// labels whose rounding would add up (a tab strip).
    pub(crate) fn text_advance(&mut self, label: &Label, spec: TextSpec) -> f32 {
        let text = label.resolve(self.tr);
        layout_text(self.res, self.env, &text, spec, None).width / self.env.scale
    }

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
    /// A dialog, sheet or panel above everything else.
    Dialog(Dialog),
    /// Rows of cells under column headers.
    Table(Table),
    /// Items in levels that open and close.
    Tree(Tree),
    /// The top bar of a window or page; its children are the controls at start and end.
    HeaderBar(HeaderBar),
    /// A QR code.
    QrCode(QrCode),
    /// Items the user drags into place on a canvas (display arrangement).
    Arrangement(Arrangement),
    /// A brick of the desktop shell (bar tile, quick tile, clock …), operated like a button.
    Brick(Brick),
    /// The month grid of a calendar.
    DayGrid(DayGrid),
    /// Pixels the application supplies (a web page, a video).
    PixelArea(PixelArea),
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
    Flex,
    Text,
    Icon,
    Button,
    IconButton,
    Switch,
    Select,
    Slider,
    TextField,
    Row,
    Card,
    Badge,
    Progress,
    Checkbox,
    RadioGroup,
    Tabs,
    InfoBar,
    EmptyState,
    List,
    ListItem,
    Scroll,
    Dialog,
    Table,
    Tree,
    HeaderBar,
    QrCode,
    Arrangement,
    Brick,
    DayGrid,
    PixelArea
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
                | Widget::Table(_)
                | Widget::Tree(_)
                | Widget::Arrangement(_)
                | Widget::DayGrid(_)
                | Widget::PixelArea(_)
        ) || matches!(self, Widget::Brick(b) if !b.is_static())
    }

    /// `true` for widgets that can show a loading state.
    pub fn can_load(&self) -> bool {
        matches!(
            self,
            Widget::Button(_)
                | Widget::IconButton(_)
                | Widget::Switch(_)
                | Widget::Select(_)
                | Widget::Checkbox(_)
                | Widget::ListItem(_)
                | Widget::Slider(_)
                | Widget::TextField(_)
        )
    }

    /// `true` when option `index` of a radio group or select cannot be chosen.
    pub fn option_unavailable(&self, index: usize) -> bool {
        match self {
            Widget::RadioGroup(group) => group.is_unavailable(index),
            Widget::Select(select) => select.is_unavailable(index),
            _ => false,
        }
    }

    /// `true` for widgets that dim themselves when disabled; the toolkit draws them as one
    /// group so the whole brick is at the disabled opacity (fill and border dimmed once).
    pub fn dims_when_disabled(&self) -> bool {
        matches!(
            self,
            Widget::Button(_)
                | Widget::IconButton(_)
                | Widget::Switch(_)
                | Widget::Select(_)
                | Widget::Slider(_)
                | Widget::TextField(_)
                | Widget::Row(_)
                | Widget::Checkbox(_)
                | Widget::RadioGroup(_)
                | Widget::Tabs(_)
                | Widget::ListItem(_)
                | Widget::Table(_)
                | Widget::Tree(_)
                | Widget::Arrangement(_)
        )
    }

    /// `true` for widgets that can show an error with a message.
    pub fn can_fail(&self) -> bool {
        matches!(
            self,
            Widget::Select(_)
                | Widget::TextField(_)
                | Widget::Checkbox(_)
                | Widget::Slider(_)
                | Widget::ListItem(_)
                | Widget::Button(_)
                | Widget::IconButton(_)
                | Widget::Switch(_)
        )
    }

    /// The layout container of this widget, if it lays out children as a flex.
    pub fn flex(&self) -> Option<&Flex> {
        match self {
            Widget::Flex(flex) => Some(flex),
            Widget::Card(card) => Some(&card.flex),
            Widget::List(list) => Some(&list.flex),
            Widget::Scroll(scroll) => Some(&scroll.flex),
            Widget::Dialog(dialog) => Some(&dialog.flex),
            Widget::HeaderBar(bar) => Some(&bar.flex),
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
        ) || matches!(self, Widget::Checkbox(c) if c.wide)
    }
}
