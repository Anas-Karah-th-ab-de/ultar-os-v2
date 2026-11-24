//! From the shell's state to widget trees: the bar and the one popup that is open.

use super::Shell;
use crate::model::time;
use crate::model::Open;
use crate::view::{self, bar::BarText, calendar::Today, Look, Target, View};
use ultar_i18n::datefmt::ClockStyle;
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
}

/// The status icons of the bar: what says something (§5.1 "Status").
pub fn status_icons(s: &Shell) -> Vec<IconId> {
    let p = &s.d.presence;
    let mut icons = vec![if p.net_service {
        IconId::Wifi
    } else {
        IconId::Offline
    }];
    if p.audio_service {
        icons.push(IconId::Volume);
    }
    if s.d.values.battery.is_some() {
        icons.push(IconId::Battery);
    }
    icons
}

/// The bar for a screen width.
pub fn build_bar(s: &Shell, look: Look, width: f32) -> Result<View, UiError> {
    let (time, date) = s
        .local_time()
        .map(|t| time::bar_lines(t, ClockStyle::H24, &s.tr))
        .unwrap_or_default();
    let text = BarText {
        time,
        date,
        status: status_icons(s),
    };
    view::bar::build(&s.d, &text, look, width, &s.tr)
}

fn first_operable(view: &mut View, preferred: Option<Target>) {
    let wanted = preferred.and_then(|t| view.node(&t));
    let candidates: Vec<_> = wanted
        .into_iter()
        .chain(view.targets.iter().map(|(n, _)| *n))
        .collect();
    for id in candidates {
        if !view.ui.is_disabled(id) && view.ui.focus(id, false).is_ok() {
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
    }
}

/// The open popup, if any, with the keyboard focus on its first control (§13 no. 62).
pub fn build_popup(s: &Shell, look: Look, screen: Size) -> Result<Option<Popup>, UiError> {
    let mut popup = match s.d.open {
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
                offset_s: i64::from(s.clock.offset_s()),
            };
            let (v, p) = view::calendar::build(&s.d, today, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Apps(category) => {
            let (v, p) = view::apps::build(&s.d, category, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Projection => {
            let (v, p) = view::projection::build(s.d.projection, look, screen, &s.tr)?;
            sheet(v, p)
        }
        Open::Overview => {
            let (v, offset) = view::overview::build(&s.d, look, screen, &s.tr)?;
            let size = view::geometry::work_area(screen).0;
            Popup {
                view: v,
                offset,
                size,
                translucent: false,
            }
        }
        Open::KeyboardMenu | Open::PowerMenu | Open::TileMenu(_) => {
            let Some((_, menu)) = super::open_menu(s) else {
                return Ok(None);
            };
            let anchor = screen.w / 2.0;
            let (v, p) = view::menu::build(menu, look, screen, anchor, &s.tr)?;
            sheet(v, p)
        }
    };
    let preferred = match s.d.open {
        Open::Overview | Open::Apps(_) => Some(Target::SearchField),
        Open::Projection => Some(Target::Mode(s.d.projection)),
        _ => None,
    };
    if !matches!(
        s.d.open,
        Open::KeyboardMenu | Open::PowerMenu | Open::TileMenu(_)
    ) {
        first_operable(&mut popup.view, preferred);
    }
    Ok(Some(popup))
}
