//! Widget trees of the phone shell, built from `ultar-ui` widgets and tokens only. Every
//! operable node is returned with its [`Click`]. The whole tree is rebuilt for a new model
//! (a few hundred nodes at most).

pub mod bars;
pub mod clock;
pub mod home;
pub mod lock;
pub mod mesh;
pub mod quick;
pub mod recents;
pub mod search;
pub mod signin;

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
    /// The radio group of the devices (the choice screen).
    pub targets: Option<NodeId>,
    /// The app grid (swipes turn its pages).
    pub grid: Option<NodeId>,
    /// The password or PIN field (sign-in).
    pub password: Option<NodeId>,
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

/// Where an app from another system comes from (`foreign.<source>-<name>`, contract
/// `ultar_bus::messages::foreign`): the i18n key of its grid name, `None` for Ultar apps.
pub fn foreign_source(id: &str) -> Option<&'static str> {
    let rest = id.strip_prefix("foreign.")?;
    Some(match rest.split('-').next()? {
        "deb" => "mobile.app.source.linux",
        "exe" | "msi" => "mobile.app.source.windows",
        "apk" => "mobile.app.source.android",
        _ => "mobile.app.source.other",
    })
}

/// The name of a grid cell: an app from another system says where it comes from, so it
/// is never mistaken for an Ultar app (docs/FREMD-APPS.md, the source mark).
pub fn grid_label(app: &App) -> Label {
    match (foreign_source(&app.id), &app.key) {
        (Some(source), None) => Label::key_with(source, &[("name", &app.name)]),
        _ => app_label(app),
    }
}

/// The name of `app` as shown in the current language.
pub fn app_name(app: &App, ui: &Ui) -> String {
    match &app.key {
        Some(key) => ui.translator().t(key).into_owned(),
        None => app.name.clone(),
    }
}

/// Which part of the screen a tree shows. On the phone the bars and the body are separate
/// compositor surfaces (the bars stay while an app is open); tests and the lock screen use
/// the whole screen in one tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// Status bar, body and navigation bar in one tree.
    Full,
    /// Only the status bar.
    Status,
    /// Only the navigation bar.
    Nav,
    /// Only what lies between the bars.
    Body,
    /// The body with room for the bars above and below it (the home screen is the
    /// background behind the bar panels).
    Background,
}

/// The status bar shows the time (not on the lock screen and the quick settings, which show
/// it large themselves).
pub fn status_time(screen: Screen) -> bool {
    !matches!(screen, Screen::Lock | Screen::Quick)
}

/// The navigation bar is there (not while locked; the choice of a device covers it in a
/// whole-screen tree, as the board's sheet does – the shell keeps its own bar surface).
pub fn has_nav(screen: Screen) -> bool {
    !matches!(screen, Screen::Lock | Screen::Mesh)
}

/// Builds `part` of `model`'s screen into `ui` (cleared first) and paints its clocks.
pub fn build(ui: &mut Ui, model: &Mobile, part: Part) -> Result<Built, UiError> {
    start(ui, |b, root| {
        let screen = model.screen;
        if matches!(part, Part::Full | Part::Status) {
            bars::status(b, root, model, status_time(screen))?;
        }
        if part == Part::Background {
            let top = Sizing::AUTO.with_height(metrics::status());
            b.add(root, Flex::row(), top)?;
        }
        if matches!(part, Part::Full | Part::Body | Part::Background) {
            body(b, root, model)?;
            if model.power_menu {
                quick::power_menu(b, root)?;
            }
        }
        if matches!(part, Part::Full | Part::Nav) && has_nav(screen) {
            bars::nav(b, root)?;
        }
        if part == Part::Background {
            b.add(root, Flex::row(), Sizing::AUTO.with_height(metrics::nav()))?;
        }
        Ok(())
    })
}

fn body(b: &mut Builder<'_>, root: NodeId, model: &Mobile) -> Result<(), UiError> {
    let shown = body_of(b, root, model);
    mesh::notice(b, root, model)?;
    shown
}

fn body_of(b: &mut Builder<'_>, root: NodeId, model: &Mobile) -> Result<(), UiError> {
    match model.screen {
        Screen::Lock => lock::body(b, root, model),
        Screen::Home => home::body(b, root, model),
        Screen::Quick => quick::body(b, root, model),
        Screen::Recents => recents::body(b, root, model),
        Screen::Search => search::body(b, root, model),
        Screen::Mesh => mesh::body(b, root, model),
    }
}

/// What the phone's lock program shows: the cover, or the sign-in while it runs.
pub fn build_lock(
    ui: &mut Ui,
    state: &ultar_lock_core::LockState,
    cover: &lock::Cover<'_>,
    status: &Mobile,
    now_ms: u64,
) -> Result<Built, UiError> {
    start(ui, |b, root| {
        bars::status(b, root, status, false)?;
        match state.stage {
            ultar_lock_core::Stage::Cover | ultar_lock_core::Stage::Hidden => {
                lock::cover(b, root, cover)
            }
            ultar_lock_core::Stage::SignIn | ultar_lock_core::Stage::Checking => {
                signin::body(b, root, state, now_ms)
            }
        }
    })
}

fn start(
    ui: &mut Ui,
    fill: impl FnOnce(&mut Builder<'_>, NodeId) -> Result<(), UiError>,
) -> Result<Built, UiError> {
    ui.clear();
    ui.set_root_flex(Flex::column());
    let root = ui.root();
    let mut b = Builder {
        ui,
        built: Built::default(),
    };
    fill(&mut b, root)?;
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

#[cfg(test)]
mod foreign_tests {
    use super::{foreign_source, grid_label};
    use crate::model::apps::App;
    use ultar_ui::icon::IconId;
    use ultar_ui::widgets::Label;

    #[test]
    fn apps_from_other_systems_say_where_they_come_from() {
        assert_eq!(
            foreign_source("foreign.deb-mousepad"),
            Some("mobile.app.source.linux")
        );
        assert_eq!(
            foreign_source("foreign.exe-notepad"),
            Some("mobile.app.source.windows")
        );
        assert_eq!(
            foreign_source("foreign.apk-notes"),
            Some("mobile.app.source.android")
        );
        assert_eq!(foreign_source("org.ultar.files"), None);
        let app = App::named("foreign.deb-mousepad", "Mousepad", IconId::Package);
        assert_eq!(
            grid_label(&app),
            Label::key_with("mobile.app.source.linux", &[("name", "Mousepad")])
        );
        let files = App::system("org.ultar.files", "files", IconId::Package);
        assert_eq!(grid_label(&files), Label::key("mobile.app.files"));
    }
}
