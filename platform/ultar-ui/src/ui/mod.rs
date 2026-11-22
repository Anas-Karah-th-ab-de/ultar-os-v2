//! The widget tree with its interaction state: build it, feed it events, paint it.

mod access;
mod measure;
mod render;
mod tree;

pub use access::{A11yNode, A11yState, Role};

use crate::geom::{Rect, Size};
use crate::icon::PathError;
use crate::layout::{Flex, LogicalRect, Sizing};
use crate::paint::{PaintEnv, Resources};
use crate::text::{TextEngine, TextError};
use crate::tokens::{motion, ColorRole, InputMode, SizeClass, Space, ThemeKind};
use crate::widgets::{Label, Widget};
use std::cell::RefCell;
use std::fmt;
use ultar_i18n::{Direction, Lang, ParseError, Translator};

/// Most nodes one tree may hold.
pub const MAX_NODES: usize = 4096;
/// Deepest nesting of nodes.
pub const MAX_DEPTH: usize = 64;
/// Most separate dirty rectangles before the whole surface is repainted.
const MAX_DIRTY: usize = 16;

/// Handle of a node in a [`Ui`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Why a UI operation was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum UiError {
    /// The text engine could not start.
    Text(TextError),
    /// The icon set could not be parsed.
    Icons(PathError),
    /// A toolkit catalog is malformed.
    Catalog(ParseError),
    /// The node does not exist in this tree.
    UnknownNode(NodeId),
    /// The node cannot have (more) children.
    NotAContainer(NodeId),
    /// The tree already holds [`MAX_NODES`] nodes.
    TooManyNodes,
    /// The tree would be nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// The node is not the kind of widget the operation needs.
    WrongWidget {
        /// The node.
        node: NodeId,
        /// The widget kind that was expected.
        expected: &'static str,
    },
    /// The widget has no such state (for example loading on a slider).
    NotApplicable(NodeId),
    /// An option index beyond the select's options.
    OptionOutOfRange {
        /// The node.
        node: NodeId,
        /// The index that was asked for.
        index: usize,
    },
    /// Scale factors must be finite and between 0.5 and 8.
    InvalidScale,
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UiError::Text(e) => write!(f, "text engine: {e}"),
            UiError::Icons(e) => write!(f, "icon set: {e}"),
            UiError::Catalog(e) => write!(f, "toolkit catalog: {e}"),
            UiError::UnknownNode(id) => write!(f, "node {} does not exist", id.0),
            UiError::NotAContainer(id) => write!(f, "node {} cannot take a child", id.0),
            UiError::TooManyNodes => write!(f, "more than {MAX_NODES} nodes"),
            UiError::TooDeep => write!(f, "nesting deeper than {MAX_DEPTH}"),
            UiError::WrongWidget { node, expected } => {
                write!(f, "node {} is not a {expected}", node.0)
            }
            UiError::NotApplicable(id) => write!(f, "node {} has no such state", id.0),
            UiError::OptionOutOfRange { node, index } => {
                write!(f, "node {} has no option {index}", node.0)
            }
            UiError::InvalidScale => write!(f, "scale must be between 0.5 and 8"),
        }
    }
}

impl std::error::Error for UiError {}

impl From<TextError> for UiError {
    fn from(e: TextError) -> UiError {
        UiError::Text(e)
    }
}

impl From<PathError> for UiError {
    fn from(e: PathError) -> UiError {
        UiError::Icons(e)
    }
}

impl From<ParseError> for UiError {
    fn from(e: ParseError) -> UiError {
        UiError::Catalog(e)
    }
}

pub(crate) struct Node {
    pub(crate) widget: Widget,
    pub(crate) sizing: Sizing,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) frame: LogicalRect,
    pub(crate) disabled: bool,
    pub(crate) loading: bool,
    pub(crate) error: Option<Label>,
    /// Last measurement: available width and resulting size.
    pub(crate) measured: Option<(f32, Size)>,
}

/// The open options list of a select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Popup {
    pub(crate) select: NodeId,
    pub(crate) highlight: usize,
    pub(crate) scroll: usize,
}

/// Which nodes the user currently interacts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InputState {
    pub(crate) hover: Option<NodeId>,
    pub(crate) pressed: Option<NodeId>,
    pub(crate) focus: Option<NodeId>,
    pub(crate) focus_visible: bool,
    pub(crate) touch: Option<u64>,
    pub(crate) popup: Option<Popup>,
}

#[derive(Default)]
struct Dirty {
    layout: bool,
    full: bool,
    rects: Vec<Rect>,
}

/// A widget tree, its layout, its interaction state and its drawing resources.
pub struct Ui {
    pub(crate) nodes: Vec<Node>,
    /// Nodes in logical (depth-first) order; rebuilt with the layout.
    pub(crate) order: Vec<NodeId>,
    pub(crate) env: PaintEnv,
    pub(crate) mode: InputMode,
    reduce_motion: bool,
    size: Size,
    background: ColorRole,
    pub(crate) tr: Translator,
    pub(crate) res: RefCell<Resources>,
    pub(crate) input: InputState,
    dirty: Dirty,
    pub(crate) now_ms: u64,
}

const TOOLKIT_CATALOGS: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../../locales/ar.ftl")),
    (Lang::En, include_str!("../../locales/en.ftl")),
    (Lang::De, include_str!("../../locales/de.ftl")),
];

/// Duration of one revolution of the progress ring.
const SPIN_MS: u64 = 4 * motion::OVERVIEW as u64;

impl Ui {
    /// A UI of `size` logical points whose root is a column. `translator` carries the app's
    /// catalogs; the toolkit's own keys (`ui.*`) are added to it.
    pub fn new(
        env: PaintEnv,
        mode: InputMode,
        size: Size,
        mut translator: Translator,
    ) -> Result<Ui, UiError> {
        if !(env.scale.is_finite() && (0.5..=8.0).contains(&env.scale)) {
            return Err(UiError::InvalidScale);
        }
        for (lang, source) in TOOLKIT_CATALOGS {
            translator.add_source(lang, source)?;
        }
        translator.set_lang(env.lang);
        let res = Resources {
            text: TextEngine::new()?,
            icons: crate::icon::IconCache::new()?,
        };
        let mut ui = Ui {
            nodes: Vec::new(),
            order: Vec::new(),
            env,
            mode,
            reduce_motion: false,
            size,
            background: ColorRole::Ground,
            tr: translator,
            res: RefCell::new(res),
            input: InputState::default(),
            dirty: Dirty::default(),
            now_ms: 0,
        };
        ui.clear();
        Ok(ui)
    }

    /// The root node (a flex container).
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    /// Removes every node and interaction state; the root becomes an empty column.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.nodes.push(tree::new_node(
            Widget::Flex(Flex::column()),
            Sizing::AUTO,
            None,
        ));
        self.input = InputState::default();
        self.invalidate_layout();
    }

    /// Replaces the layout of the root container.
    pub fn set_root_flex(&mut self, flex: Flex) {
        self.nodes[0].widget = Widget::Flex(flex);
        self.invalidate_layout();
    }

    /// The paint environment (language, direction, theme, scale).
    pub fn env(&self) -> &PaintEnv {
        &self.env
    }

    /// The translator with the app's and the toolkit's catalogs.
    pub fn translator(&self) -> &Translator {
        &self.tr
    }

    /// Mutable access to the translator, to add catalogs. Call [`Ui::invalidate_layout`]
    /// afterwards if visible texts may have changed.
    pub fn translator_mut(&mut self) -> &mut Translator {
        &mut self.tr
    }

    /// The surface size in logical points.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The size class of the surface (compact, medium, wide) by its logical width.
    pub fn size_class(&self) -> SizeClass {
        SizeClass::for_width(self.size.w)
    }

    /// The input mode that sizes the targets.
    pub fn input_mode(&self) -> InputMode {
        self.mode
    }

    /// Resizes the surface.
    pub fn set_size(&mut self, size: Size) {
        if size != self.size {
            self.size = size;
            self.invalidate_layout();
        }
    }

    /// Switches language; the direction follows the language.
    pub fn set_lang(&mut self, lang: Lang) {
        self.env.lang = lang;
        self.env.direction = lang.direction();
        self.tr.set_lang(lang);
        self.invalidate_layout();
    }

    /// Overrides the layout direction without changing the language. Nothing is laid out
    /// again: logical rectangles are resolved for the new direction when painting.
    pub fn set_direction(&mut self, direction: Direction) {
        if self.env.direction != direction {
            self.env.direction = direction;
            self.dirty.full = true;
        }
    }

    /// Switches the theme.
    pub fn set_theme(&mut self, theme: ThemeKind) {
        if self.env.theme != theme {
            self.env.theme = theme;
            self.dirty.full = true;
        }
    }

    /// Sets the device pixels per logical point (0.5 to 8).
    pub fn set_scale(&mut self, scale: f32) -> Result<(), UiError> {
        if !(scale.is_finite() && (0.5..=8.0).contains(&scale)) {
            return Err(UiError::InvalidScale);
        }
        self.env.scale = scale;
        self.invalidate_layout();
        Ok(())
    }

    /// Switches between pointer and touch target sizes.
    pub fn set_input_mode(&mut self, mode: InputMode) {
        if self.mode != mode {
            self.mode = mode;
            self.invalidate_layout();
        }
    }

    /// The colour role painted behind the tree.
    pub fn set_background(&mut self, role: ColorRole) {
        self.background = role;
        self.dirty.full = true;
    }

    /// "Reduce motion": knobs jump and progress rings stand still.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        if self.reduce_motion != reduce {
            self.reduce_motion = reduce;
            self.dirty.full = true;
        }
    }

    /// Sets the clock (milliseconds, monotonic, supplied by the caller) that drives
    /// animations, and marks animating widgets for repaint.
    pub fn advance_to(&mut self, now_ms: u64) {
        if now_ms == self.now_ms {
            return;
        }
        let was_animating = self.animating();
        self.now_ms = now_ms;
        if !(was_animating || self.animating()) {
            return;
        }
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            if node.loading || matches!(&node.widget, Widget::Switch(s) if s.toggled_at.is_some()) {
                self.touch(NodeId(index as u32));
            }
        }
        for node in &mut self.nodes {
            if let Widget::Switch(switch) = &mut node.widget {
                let done = |t: u64| now_ms.saturating_sub(t) >= u64::from(motion::FEEDBACK);
                if switch.toggled_at.is_some_and(done) {
                    switch.toggled_at = None;
                }
            }
        }
    }

    /// `true` while something moves (a knob in transit, a progress ring), i.e. the caller
    /// should keep calling [`Ui::advance_to`] and [`Ui::paint`] every frame.
    pub fn animating(&self) -> bool {
        if self.reduce_motion {
            return false;
        }
        self.nodes.iter().any(|node| {
            node.loading || matches!(&node.widget, Widget::Switch(s) if s.toggled_at.is_some())
        })
    }

    /// Rotation of progress rings for the current clock, 0..1.
    pub(crate) fn spin(&self) -> f32 {
        if self.reduce_motion {
            0.0
        } else {
            (self.now_ms % SPIN_MS) as f32 / SPIN_MS as f32
        }
    }

    /// Knob position of a switch for the current clock, 0 (off) to 1 (on).
    pub(crate) fn knob_position(&self, on: bool, toggled_at: Option<u64>) -> f32 {
        let target = if on { 1.0 } else { 0.0 };
        match toggled_at {
            Some(start) if !self.reduce_motion => {
                let elapsed = self.now_ms.saturating_sub(start) as f32 / motion::FEEDBACK as f32;
                let progress = elapsed.clamp(0.0, 1.0);
                if on {
                    progress
                } else {
                    1.0 - progress
                }
            }
            _ => target,
        }
    }

    pub(crate) fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Forces a new layout and a full repaint (after catalog changes).
    pub fn invalidate_layout(&mut self) {
        self.dirty.layout = true;
        self.dirty.full = true;
    }

    /// Requests a repaint of the whole surface without a new layout.
    pub(crate) fn repaint_all(&mut self) {
        self.dirty.full = true;
    }

    /// `true` when [`Ui::paint`] has something to draw.
    pub fn needs_paint(&self) -> bool {
        self.dirty.full || !self.dirty.rects.is_empty()
    }

    /// Marks a logical-point rectangle for repaint.
    pub(crate) fn damage(&mut self, rect: Rect) {
        if self.dirty.full || rect.is_empty() {
            return;
        }
        if self.dirty.rects.len() >= MAX_DIRTY {
            self.dirty.full = true;
            self.dirty.rects.clear();
        } else {
            self.dirty.rects.push(rect);
        }
    }

    /// Marks a node (with its focus ring and halo) for repaint.
    pub(crate) fn touch(&mut self, id: NodeId) {
        if let Some(rect) = self.rect(id) {
            self.damage(rect.inflate(Space::S8.points()));
        }
    }

    /// The physical rectangle of a node in logical points, for the current direction.
    /// Meaningful after [`Ui::layout`].
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        let node = self.nodes.get(id.index())?;
        Some(node.frame.resolve(self.env.direction, self.size.w))
    }

    /// The logical rectangle of a node (start-relative), independent of the direction.
    pub fn logical_rect(&self, id: NodeId) -> Option<LogicalRect> {
        self.nodes.get(id.index()).map(|n| n.frame)
    }
}
