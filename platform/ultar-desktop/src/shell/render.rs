//! From the shell's state to widget trees: the bar and the one popup that is open.

use super::Shell;
use crate::model::time;
use crate::model::Open;
use crate::view::{self, bar::BarText, calendar::Today, Look, Target, View};
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::ui::UiError;

/// The open popup of the bar.
#[derive(Debug)]
pub struct Popup {
    /// Its tree.
    pub view: View,
    /// Position relative to the bar's panel (logical points).
    pub offset: (i32, i32),
    /// Its size.
    pub size: Size,
    /// Pixels with alpha (floating cards with shadow).
    pub translucent: bool,
    /// Margin above the card inside the surface (its shadow's room).
    pub inset_top: f32,
}

/// The network icon of the bar, from `net.state`: the cable while a
/// wired interface has an address, Wi-Fi while connected to a Wi-Fi network, otherwise
/// offline; Wi-Fi as before while the service has not reported yet.
fn net_icon(s: &Shell) -> IconId {
    use ultar_bus::messages::net::LinkState;
    if !s.d.presence.net_service {
        return IconId::Offline;
    }
    let Some(net) = s.d.svc.net.as_ref() else {
        return IconId::Wifi;
    };
    let wifi = |l: &ultar_bus::messages::net::NetLink| l.state == LinkState::Connected;
    if net.wired_connected() {
        IconId::Cable
    } else if net.wifi_link.as_ref().is_some_and(wifi) || net.ssid.is_some() {
        IconId::Wifi
    } else if net.ethernet.is_empty() && net.wifi_link.is_none() && net.wired.is_some() {
        // a service of the first contract version: only the cable flag
        IconId::Cable
    } else {
        IconId::Offline
    }
}

/// The status icons of the bar: what says something (§5.1 "Status").
pub fn status_icons(s: &Shell) -> Vec<IconId> {
    let p = &s.d.presence;
    let mut icons = vec![net_icon(s)];
    if p.audio_service {
        icons.push(IconId::Volume);
    }
    if s.d.values.battery.is_some() {
        icons.push(IconId::Battery);
    }
    icons
}

/// The time and date lines and the status icons of the bar.
fn bar_text(s: &Shell) -> BarText {
    let (time, date) = s
        .local_time()
        .map(|t| time::bar_lines_with(t, s.d.prefs.clock, &s.tr))
        .unwrap_or_default();
    BarText {
        time,
        date,
        status: status_icons(s),
    }
}

/// The shell's first panel for a screen width: the bar at the bottom or the top bar of the
/// variant with a dock.
pub fn build_bar(s: &Shell, look: Look, width: f32) -> Result<View, UiError> {
    let text = bar_text(s);
    if s.shape.is_top() {
        return view::dock::top_bar(&s.d, &text, look, width, &s.tr);
    }
    view::bar::build(&s.d, &text, look, width, &s.tr)
}

fn first_operable(view: &mut View, preferred: Option<Target>) {
    let wanted = preferred.and_then(|t| view.node(&t));
    let candidates: Vec<_> = wanted
        .into_iter()
        .chain(view.targets.iter().map(|(n, _)| *n))
        .collect();
    for id in candidates {
        if !view.ui.is_disabled(id) && view.ui.focus(id).is_ok() {
            return;
        }
    }
}

fn sheet(view: View, placement: view::geometry::Placement) -> Popup {
    Popup {
        view,
        offset: placement.offset,
        size: placement.surface,
        translucent: true,
        inset_top: placement.top.points(),
    }
}

/// The open popup, if any, with the keyboard focus on its first control (§13 no. 62).
pub fn build_popup(s: &Shell, look: Look, screen: Size) -> Result<Option<Popup>, UiError> {
    let Some(mut popup) = desktop_popup(s, look, screen)? else {
        return Ok(None);
    };
    if s.shape.is_top() {
        super::shapes::for_top_bar(s.d.open, &mut popup, screen);
    }
    let preferred = match s.d.open {
        Open::Overview | Open::Apps(_) | Open::Helper => Some(Target::SearchField),
        Open::Projection(mode) => Some(Target::Mode(mode)),
        _ => None,
    };
    if !matches!(
        s.d.open,
        Open::KeyboardMenu
            | Open::PowerMenu
            | Open::OutputMenu
            | Open::DriveMenu(_)
            | Open::TileMenu(_)
            | Open::AppMenu
    ) {
        first_operable(&mut popup.view, preferred);
    }
    Ok(Some(popup))
}

/// The popup of a menu that is open.
fn menu_popup(s: &Shell, look: Look, screen: Size, anchor: f32) -> Result<Option<Popup>, UiError> {
    let Some((_, menu)) = super::open_menu(s) else {
        return Ok(None);
    };
    let (v, p) = view::menu::build(menu, look, screen, anchor, &s.tr)?;
    Ok(Some(sheet(v, p)))
}

fn desktop_popup(s: &Shell, look: Look, screen: Size) -> Result<Option<Popup>, UiError> {
    let popup = match s.d.open {
        Open::None => return Ok(None),
        Open::Quick(page) => {
            let (v, p) = view::quick::build(&s.d, page, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Calendar => {
            let Some(t) = s.local_time() else {
                return Ok(None);
            };
            let today = Today {
                date: t.date,
                now_ms: s.clock.now_ms(),
                offset_s: i64::from(s.offset_s()),
            };
            let (v, p) = view::calendar::build(&s.d, today, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Apps(category) => {
            let (v, p) = view::apps::build(&s.d, category, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Projection(mode) => {
            let (v, p) = view::projection::build(mode, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Helper => {
            let (v, p) = view::helper::build(&s.d, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Overview => {
            let (size, offset) = super::shapes::overview_area(s.shape, screen);
            Popup {
                view: view::overview::build(&s.d, look, size, &s.tr)?,
                offset,
                size,
                translucent: false,
                inset_top: 0.0,
            }
        }
        Open::KeyboardMenu
        | Open::PowerMenu
        | Open::OutputMenu
        | Open::DriveMenu(_)
        | Open::TileMenu(_)
        | Open::AppMenu => {
            let anchor = super::shapes::menu_anchor(s, look, screen);
            return menu_popup(s, look, screen, anchor);
        }
    };
    Ok(Some(popup))
}
