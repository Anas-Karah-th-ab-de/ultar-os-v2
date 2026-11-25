//! The bars every screen shares: the status bar at the top (time at the start, radios and
//! battery at the end) and the navigation bar at the bottom (recent apps · home · back).

use super::{metrics, Builder};
use crate::click::Click;
use crate::model::Mobile;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, IconButton, Label, Text};

/// The status icons in reading order: Wi-Fi, mobile signal, airplane mode.
pub fn status_icons(m: &Mobile) -> Vec<IconId> {
    let mut icons = Vec::new();
    if m.quick.is_on(crate::model::quick::Toggle::Airplane) {
        icons.push(IconId::Plane);
    }
    if m.status.wifi.is_some() {
        icons.push(IconId::Wifi);
    } else if !m.quick.is_on(crate::model::quick::Toggle::Wifi) {
        icons.push(IconId::Offline);
    }
    if m.hardware.modem && m.status.cell.is_some() {
        icons.push(IconId::Cell);
    }
    icons
}

/// The status bar; `time` = show the time at the start (not on the lock screen, which shows
/// it large). The status part opens the quick settings.
pub fn status(b: &mut Builder<'_>, root: NodeId, m: &Mobile, time: bool) -> Result<(), UiError> {
    let flex = Flex::row()
        .main(MainAlign::SpaceBetween)
        .cross(CrossAlign::Center)
        .padding(Padding::symmetric(Space::S12, Space::S4));
    let bar = b.flex(root, flex, Sizing::AUTO.with_height(metrics::status()))?;
    let clock = match (time, m.now) {
        (true, Some(t)) => t.clock(),
        _ => String::new(),
    };
    let spec = TextSpec::new(TypeSize::S12).weight(Weight::Semibold);
    b.add(bar, Text::new(Label::data(&clock)).spec(spec), Sizing::AUTO)?;
    let level = m
        .status
        .battery
        .map(|p| Label::key_with("mobile.percent", &[("value", &p.to_string())]));
    let brick = Brick::new(
        BrickKind::PhoneStatus {
            icons: status_icons(m),
            level,
            open: m.screen == crate::model::Screen::Quick,
        },
        Label::key("mobile.status"),
    );
    let sizing = Sizing::AUTO.with_height(metrics::status());
    // Locked: shown, but the quick settings stay closed until the person is recognised.
    if m.locked {
        b.add(bar, brick, sizing)?;
    } else {
        b.click(bar, brick, sizing, Click::Status)?;
    }
    Ok(())
}

/// The navigation bar: recent apps, home screen, back – evenly spread.
pub fn nav(b: &mut Builder<'_>, root: NodeId) -> Result<(), UiError> {
    let flex = Flex::row()
        .main(MainAlign::SpaceBetween)
        .cross(CrossAlign::Center);
    let bar = b.flex(root, flex, Sizing::AUTO.with_height(metrics::nav()))?;
    for (icon, key, click) in [
        (IconId::Workspaces, "mobile.nav.recents", Click::Recents),
        (IconId::Home, "mobile.nav.home", Click::Home),
        (IconId::Back, "mobile.nav.back", Click::Back),
    ] {
        b.click(
            bar,
            IconButton::new(icon, Label::key(key)),
            Sizing::AUTO,
            click,
        )?;
    }
    Ok(())
}

/// The page dots under the app grid: the shown page long, the others short.
pub fn dots(
    b: &mut Builder<'_>,
    parent: NodeId,
    pages: usize,
    shown: usize,
) -> Result<(), UiError> {
    if pages < 2 {
        return Ok(());
    }
    let row = b.flex(
        parent,
        Flex::row().main(MainAlign::Center).gap(Space::S4),
        Sizing::AUTO.with_height(Space::S12.points()),
    )?;
    for page in 0..pages {
        let width = if page == shown {
            Space::S16.points()
        } else {
            Space::S4.points()
        };
        let number = (page + 1).to_string();
        let brick = Brick::new(
            BrickKind::Handle { width },
            Label::key_with("mobile.page", &[("number", &number)]),
        );
        let sizing = Sizing::width(Space::S24.points()).with_height(Space::S12.points());
        b.click(row, brick, sizing, Click::Page(page))?;
    }
    Ok(())
}
