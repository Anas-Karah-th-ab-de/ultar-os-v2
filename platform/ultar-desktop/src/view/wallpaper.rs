//! The desktop background (the system picture "ultar-noir" of `appearance.wallpaper.*`):
//! the ground of the theme with the company logo of KM Code Craft in the middle of the work
//! area, quiet, so it never competes with the windows above it (docs/BRAND.md §4), and the
//! person's app shortcuts over it (`view::desk`).

use super::{new_ui, Look, View};
use crate::model::{Desktop, Shape};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Sizing};
use ultar_ui::tokens::ColorRole;
use ultar_ui::ui::UiError;
use ultar_ui::widgets::{Brick, BrickKind, Label};

/// Share of the shorter screen side the logo takes (like the first desktop).
const SHARE: f32 = 0.34;
/// Largest logo width in points.
const MAX_WIDTH: f32 = 420.0;

/// Width of the background's logo on a screen (points).
pub fn logo_width(screen: Size) -> u16 {
    (screen.w.min(screen.h) * SHARE).clamp(0.0, MAX_WIDTH) as u16
}

/// Builds the background for the whole screen; the logo stands in the middle of what the
/// bar (and the dock) leave free, the shortcuts in that area from their corner.
pub fn build(
    d: &Desktop,
    look: Look,
    screen: Size,
    shape: Shape,
    tr: &Translator,
) -> Result<View, UiError> {
    let mut ui = new_ui(look, screen, tr)?;
    ui.set_background(ColorRole::Ground);
    let (top, bottom) = if shape.is_top() {
        (crate::view::dock::TOP_HEIGHT, crate::view::dock::DOCK_PANEL)
    } else {
        (0.0, ultar_ui::tokens::size::BAR_HEIGHT)
    };
    ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
    let root = ui.root();
    // Room of the bar and the dock above and below; the logo centred in what is left, the
    // shortcuts over it.
    ui.add_sized(root, Flex::column(), Sizing::AUTO.with_height(top))?;
    let area = ui.add_sized(root, Flex::stack(), Sizing::grow(1))?;
    ui.add_sized(root, Flex::column(), Sizing::AUTO.with_height(bottom))?;
    let middle = Flex::column()
        .main(MainAlign::Center)
        .cross(CrossAlign::Center);
    let middle = ui.add_sized(area, middle, Sizing::grow(1))?;
    let logo = Brick::new(
        BrickKind::Logo {
            width: logo_width(screen),
            quiet: true,
        },
        Label::key("brand.label.logo"),
    );
    ui.add(middle, logo)?;
    let mut view = View::new(ui);
    let height = (screen.h - top - bottom).max(0.0);
    super::desk::shortcuts(&mut view, area, d, (look, height), tr)?;
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;
    use ultar_ui::tokens::ThemeKind;

    #[test]
    fn the_logo_follows_the_screen_and_stays_moderate() {
        assert_eq!(logo_width(Size::new(1280.0, 800.0)), 272);
        assert_eq!(logo_width(Size::new(3840.0, 2160.0)), 420);
        let tr = crate::i18n::translator(Lang::En).expect("catalogs");
        let look = Look::new(Lang::En, ThemeKind::Dark);
        let mut d = Desktop::new(Lang::En);
        let screen = Size::new(1280.0, 800.0);
        use crate::model::places::DeskItem;
        use crate::view::Target;
        let targets = |v: &View| v.targets.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>();
        let view = build(&d, look, screen, Shape::default(), &tr).expect("view");
        assert_eq!(targets(&view), vec![Target::Desk(DeskItem::Home)]);
        d.places.add("org.ultar.files");
        d.places.add("org.ultar.not.installed");
        let view = build(&d, look, screen, Shape::default(), &tr).expect("view");
        assert_eq!(
            targets(&view),
            vec![
                Target::Desk(DeskItem::Home),
                Target::Desk(DeskItem::Shortcut(0))
            ],
            "only installed apps are shown"
        );
        d.prefs.desk.shown = false;
        let view = build(&d, look, screen, Shape::TOP, &tr).expect("view");
        assert!(
            view.targets.is_empty(),
            "desktop.icons.enabled off hides them"
        );
    }
}
