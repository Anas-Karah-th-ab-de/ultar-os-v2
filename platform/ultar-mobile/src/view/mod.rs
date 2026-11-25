//! Widget trees of the phone shell, built from `ultar-ui` widgets and tokens only. Every
//! operable node is returned with its [`Click`]. The whole tree is rebuilt for a new model
//! (a few hundred nodes at most).

pub mod bars;
pub mod clock;
pub mod home;
pub mod lock;
pub mod quick;
pub mod recents;
pub mod search;

use crate::click::Click;
use crate::model::apps::App;
use crate::model::{Mobile, Screen};
use ultar_ui::canvas::Canvas;
use ultar_ui::geom::Size;
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::size;
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Label, Widget};

/// Geometry of the phone shell (base grid × n).
pub mod metrics {
    use ultar_ui::tokens::{size, Space};

    /// n units of the base grid (4 points).
    pub fn grid(n: f32) -> f32 {
        Space::S4.points() * n
    }
    /// Height of the status bar.
    pub fn status() -> f32 {
        Space::S32.points()
    }
    /// Height of the navigation bar (a touch target).
    pub fn nav() -> f32 {
        size::TARGET_TOUCH_MIN
    }
    /// App columns of the grid and the dock.
    pub const COLUMNS: usize = 4;
    /// Height of an app cell of the grid (icon box, name).
    pub fn cell() -> f32 {
        grid(24.0)
    }
}

/// Rows of the app grid on a screen `height` points high (the old phone: three rows on a
/// 760 point screen, four from 820).
pub fn grid_rows(height: f32) -> usize {
    if height >= metrics::grid(205.0) {
        4
    } else {
        3
    }
}

/// Apps on one page of the grid.
pub fn per_page(height: f32) -> usize {
    grid_rows(height) * metrics::COLUMNS
}

/// Handles of one built tree.
#[derive(Default)]
pub struct Built {
    /// Operable nodes and what they mean.
    pub clicks: Vec<(NodeId, Click)>,
    /// The brightness slider.
    pub brightness: Option<NodeId>,
    /// The volume slider.
    pub volume: Option<NodeId>,
    /// The search field (search screen).
    pub search: Option<NodeId>,
    /// The app grid (swipes turn its pages).
    pub grid: Option<NodeId>,
    /// Dot-matrix pictures to paint after layout: the node and the time.
    pub clocks: Vec<(NodeId, String)>,
}

impl Built {
    /// What `node` means.
    pub fn click(&self, node: NodeId) -> Option<&Click> {
        self.clicks.iter().find(|(n, _)| *n == node).map(|(_, c)| c)
    }

    /// The node of `click` (tests, keyboard focus).
    pub fn node(&self, click: &Click) -> Option<NodeId> {
        self.clicks
            .iter()
            .find(|(_, c)| c == click)
            .map(|(n, _)| *n)
    }
}

/// Collects nodes while a tree is built.
pub struct Builder<'a> {
    /// The tree.
    pub ui: &'a mut Ui,
    /// What was collected.
    pub built: Built,
}

impl Builder<'_> {
    /// Adds a node.
    pub fn add(
        &mut self,
        parent: NodeId,
        widget: impl Into<Widget>,
        sizing: Sizing,
    ) -> Result<NodeId, UiError> {
        self.ui.add_sized(parent, widget, sizing)
    }

    /// Adds an operable node.
    pub fn click(
        &mut self,
        parent: NodeId,
        widget: impl Into<Widget>,
        sizing: Sizing,
        click: Click,
    ) -> Result<NodeId, UiError> {
        let id = self.ui.add_sized(parent, widget, sizing)?;
        self.built.clicks.push((id, click));
        Ok(id)
    }

    /// Adds a flex container.
    pub fn flex(&mut self, parent: NodeId, flex: Flex, sizing: Sizing) -> Result<NodeId, UiError> {
        self.ui.add_sized(parent, flex, sizing)
    }

    /// An empty node that takes the left-over space.
    pub fn spacer(&mut self, parent: NodeId) -> Result<NodeId, UiError> {
        self.ui.add_sized(parent, Flex::column(), Sizing::grow(1))
    }

    /// The screen's size in points.
    pub fn screen(&self) -> Size {
        self.ui.size()
    }
}

/// The name of `app` as a label (catalog key of a system app, else its own name).
pub fn app_label(app: &App) -> Label {
    match &app.key {
        Some(key) => Label::key(key),
        None => Label::data(&app.name),
    }
}

/// The name of `app` as shown in the current language.
pub fn app_name(app: &App, ui: &Ui) -> String {
    match &app.key {
        Some(key) => ui.translator().t(key).into_owned(),
        None => app.name.clone(),
    }
}

/// Builds the tree of `model` into `ui` (cleared first) and paints the dot-matrix clocks.
pub fn build(ui: &mut Ui, model: &Mobile) -> Result<Built, UiError> {
    ui.clear();
    ui.set_root_flex(Flex::column());
    let root = ui.root();
    let mut b = Builder {
        ui,
        built: Built::default(),
    };
    match model.screen {
        Screen::Lock => lock::build(&mut b, root, model)?,
        Screen::Home => home::build(&mut b, root, model)?,
        Screen::Quick => quick::build(&mut b, root, model)?,
        Screen::Recents => recents::build(&mut b, root, model)?,
        Screen::Search => search::build(&mut b, root, model)?,
    }
    let built = b.built;
    paint_clocks(ui, &built)?;
    Ok(built)
}

fn paint_clocks(ui: &mut Ui, built: &Built) -> Result<(), UiError> {
    if built.clocks.is_empty() {
        return Ok(());
    }
    ui.layout();
    let env = *ui.env();
    for (id, time) in &built.clocks {
        let Some(rect) = ui.rect(*id) else { continue };
        let w = (rect.w * env.scale).round().max(1.0) as u32;
        let h = (rect.h * env.scale).round().max(1.0) as u32;
        let Ok(mut canvas) = Canvas::new(w, h, ultar_ui::canvas::PixelFormat::Bgrx8) else {
            continue;
        };
        clock::paint(&mut canvas, time, &env);
        ui.set_picture(*id, canvas)?;
    }
    Ok(())
}

/// Smallest touch target (points).
pub const TOUCH: f32 = size::TARGET_TOUCH_MIN;
