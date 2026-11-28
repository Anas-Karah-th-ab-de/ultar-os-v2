//! The desktop background (the system picture "ultar-noir" of `appearance.wallpaper.*`):
//! the ground of the theme with the company logo of KM Code Craft in the middle of the work
//! area, quiet, so it never competes with the windows above it (docs/BRAND.md §4).

use super::{new_ui, Look, View};
use crate::model::Shape;
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
/// bar (and the dock) leave free.
pub fn build(look: Look, screen: Size, shape: Shape, tr: &Translator) -> Result<View, UiError> {
    let mut ui = new_ui(look, screen, tr)?;
    ui.set_background(ColorRole::Ground);
    let (top, bottom) = if shape.is_top() {
        (crate::view::dock::TOP_HEIGHT, crate::view::dock::DOCK_PANEL)
    } else {
        (0.0, ultar_ui::tokens::size::BAR_HEIGHT)
    };
    ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
    let root = ui.root();
    // Room of the bar and the dock above and below; the logo centred in what is left.
    ui.add_sized(root, Flex::column(), Sizing::AUTO.with_height(top))?;
    let area = ui.add_sized(
        root,
        Flex::column()
            .main(MainAlign::Center)
            .cross(CrossAlign::Center),
        Sizing::grow(1),
    )?;
    ui.add_sized(root, Flex::column(), Sizing::AUTO.with_height(bottom))?;
    let logo = Brick::new(
        BrickKind::Logo {
            width: logo_width(screen),
            quiet: true,
        },
        Label::key("brand.label.logo"),
    );
    ui.add(area, logo)?;
    Ok(View::new(ui))
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
        let view = build(look, Size::new(1280.0, 800.0), Shape::default(), &tr).expect("view");
        assert!(
            view.targets.is_empty(),
            "the background has nothing to operate"
        );
    }
}
