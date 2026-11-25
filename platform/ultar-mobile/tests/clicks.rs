//! Every operable node of every screen, tapped like a finger would, with its effect; the
//! gestures of the host (swipe up to unlock, down for the quick settings, sideways for the
//! pages); and that no tap ever fails or leaves the shell without a tree.

mod common;

use common::*;
use ultar_mobile::click::Click;
use ultar_mobile::model::quick::Toggle;
use ultar_mobile::model::Screen;
use ultar_mobile::Effect;
use ultar_ui::geom::Point;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Lang;

fn home(lang: Lang) -> ultar_mobile::MobileApp {
    app(lang, ThemeKind::Dark, sample(false))
}

#[test]
fn the_lock_screen_asks_the_platform_and_only_then_opens() {
    let mut a = app(Lang::Ar, ThemeKind::Dark, sample(true));
    assert_eq!(tap(&mut a, &Click::Unlock), vec![Effect::Unlock]);
    assert_eq!(
        a.model.screen,
        Screen::Lock,
        "the tap alone does not unlock"
    );
    assert_eq!(tap(&mut a, &Click::Emergency), vec![Effect::Emergency]);
    assert_eq!(tap(&mut a, &Click::Camera), vec![Effect::Camera]);
    assert_eq!(
        tap(&mut a, &Click::Torch),
        vec![Effect::Set(Toggle::Torch, true)]
    );
    assert!(a.model.quick.is_on(Toggle::Torch));
    // Locked, the status bar opens nothing and there is no navigation bar.
    assert!(a.built().node(&Click::Status).is_none());
    assert!(a.built().node(&Click::Home).is_none());
    let swipe = a
        .swipe(Point::new(180.0, 700.0), Point::new(0.0, -200.0))
        .unwrap();
    assert_eq!(swipe, vec![Effect::Unlock]);
    a.unlocked().unwrap();
    assert_eq!(a.model.screen, Screen::Home);
    a.lock().unwrap();
    assert_eq!(a.model.screen, Screen::Lock);
}

#[test]
fn an_app_of_the_grid_and_of_the_dock_starts_and_becomes_recent() {
    for lang in [Lang::Ar, Lang::En] {
        let mut a = home(lang);
        let id = "org.ultar.settings".to_owned();
        assert_eq!(
            tap(&mut a, &Click::App(id.clone())),
            vec![Effect::Launch(id.clone())]
        );
        assert_eq!(a.model.recents.first(), Some(&id));
        let phone = "org.ultar.phone".to_owned();
        assert_eq!(
            tap(&mut a, &Click::App(phone.clone())),
            vec![Effect::Launch(phone.clone())]
        );
        assert_eq!(a.model.recents.first(), Some(&phone));
    }
}

#[test]
fn every_grid_app_is_reachable_through_the_pages() {
    let mut m = sample(false);
    for i in 0..20 {
        m.apps.push(ultar_mobile::model::apps::App::named(
            &format!("x.app{i}"),
            &format!("App {i}"),
            ultar_ui::icon::IconId::Apps,
        ));
    }
    let mut a = app(Lang::En, ThemeKind::Dark, m);
    let grid: Vec<String> = a.model.grid_apps().iter().map(|g| g.id.clone()).collect();
    let pages = a.model.page_count(ultar_mobile::view::per_page(PHONE.h));
    assert!(pages >= 2);
    let mut seen = Vec::new();
    for p in 0..pages {
        tap(&mut a, &Click::Page(p));
        assert_eq!(a.model.page, p);
        for g in &grid {
            if a.built().node(&Click::App(g.clone())).is_some() {
                seen.push(g.clone());
            }
        }
    }
    seen.sort();
    seen.dedup();
    let mut want = grid.clone();
    want.sort();
    assert_eq!(seen, want);
    // Sideways swipes: English goes on to the left, Arabic to the right.
    tap(&mut a, &Click::Page(0));
    a.swipe(Point::new(300.0, 400.0), Point::new(-120.0, 0.0))
        .unwrap();
    assert_eq!(a.model.page, 1);
    let mut ar = app(Lang::Ar, ThemeKind::Dark, a.model.clone());
    ar.swipe(Point::new(60.0, 400.0), Point::new(120.0, 0.0))
        .unwrap();
    assert_eq!(ar.model.page, 2.min(pages - 1));
}

#[test]
fn the_status_bar_and_the_swipe_down_open_the_quick_settings() {
    let mut a = home(Lang::En);
    tap(&mut a, &Click::Status);
    assert_eq!(a.model.screen, Screen::Quick);
    tap(&mut a, &Click::Status);
    assert_eq!(a.model.screen, Screen::Home);
    a.swipe(Point::new(180.0, 10.0), Point::new(0.0, 200.0))
        .unwrap();
    assert_eq!(a.model.screen, Screen::Quick);
    a.swipe(Point::new(180.0, 600.0), Point::new(0.0, -200.0))
        .unwrap();
    assert_eq!(a.model.screen, Screen::Home);
    // A swipe down in the middle of the screen is not the quick settings.
    a.swipe(Point::new(180.0, 400.0), Point::new(0.0, 200.0))
        .unwrap();
    assert_eq!(a.model.screen, Screen::Home);
}

#[test]
fn every_switch_of_the_quick_settings_switches_and_says_so() {
    let mut a = home(Lang::Ar);
    tap(&mut a, &Click::Status);
    for t in Toggle::ALL {
        let before = a.model.quick.is_on(t);
        let effects = tap(&mut a, &Click::Toggle(t));
        let after = a.model.quick.is_on(t);
        assert_eq!(effects, vec![Effect::Set(t, after)], "{t:?}");
        assert_ne!(before, after, "{t:?}");
        assert_eq!(a.model.screen, Screen::Quick);
    }
    assert_eq!(
        tap(&mut a, &Click::ToggleMore(Toggle::Wifi)),
        vec![Effect::Settings(Some("network/wifi"))]
    );
    assert_eq!(tap(&mut a, &Click::Settings), vec![Effect::Settings(None)]);
    assert!(tap(&mut a, &Click::Power).is_empty());
    assert!(a.model.power_menu);
    assert_eq!(tap(&mut a, &Click::PowerRestart), vec![Effect::Restart]);
    assert!(!a.model.power_menu);
    tap(&mut a, &Click::Power);
    assert_eq!(tap(&mut a, &Click::PowerOff), vec![Effect::PowerOff]);
    tap(&mut a, &Click::Power);
    assert_eq!(tap(&mut a, &Click::LockNow), vec![Effect::LockNow]);
    tap(&mut a, &Click::Power);
    tap(&mut a, &Click::PowerCancel);
    assert!(!a.model.power_menu);
    // Escape is the menu's cancel.
    tap(&mut a, &Click::Power);
    a.handle(ultar_ui::event::Event::Key {
        key: ultar_ui::event::Key::Escape,
        modifiers: ultar_ui::event::Modifiers::NONE,
    })
    .unwrap();
    assert!(!a.model.power_menu);
    // The navigation bar's back (its own surface on the phone) closes the menu first.
    let mut m = a.model.clone();
    m.power_menu = true;
    let mut fx = Vec::new();
    ultar_mobile::app::apply_click(&mut m, Click::Back, 12, &mut fx);
    assert_eq!((m.power_menu, m.screen), (false, Screen::Quick));
}

#[test]
fn without_a_modem_there_is_no_mobile_data() {
    let mut m = sample(false);
    m.hardware.modem = false;
    m.go(Screen::Quick);
    let a = app(Lang::En, ThemeKind::Dark, m);
    assert!(a.built().node(&Click::Toggle(Toggle::MobileData)).is_none());
    assert!(a.built().node(&Click::Toggle(Toggle::Wifi)).is_some());
}

#[test]
fn the_sliders_report_their_value() {
    let mut a = home(Lang::En);
    tap(&mut a, &Click::Status);
    let id = a.built().brightness.unwrap();
    a.ui().layout();
    let r = a.ui().rect(id).unwrap();
    let effects = tap_at(&mut a, Point::new(r.x + r.w - 2.0, r.center().y));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Brightness(v) if *v >= 90)),
        "{effects:?}"
    );
}

#[test]
fn notifications_are_dismissed_one_by_one_and_all_at_once() {
    let mut a = home(Lang::En);
    tap(&mut a, &Click::Status);
    assert_eq!(tap(&mut a, &Click::Dismiss(1)), vec![Effect::Dismiss(1)]);
    assert_eq!(a.model.notes.len(), 1);
    assert_eq!(
        tap(&mut a, &Click::ClearAll),
        vec![Effect::ClearNotes(vec![2])]
    );
    assert!(a.model.notes.is_empty());
    assert!(a.built().node(&Click::ClearAll).is_none());
}

#[test]
fn recents_open_close_and_clear() {
    let mut a = home(Lang::En);
    tap(&mut a, &Click::Recents);
    assert_eq!(a.model.screen, Screen::Recents);
    let files = "org.ultar.files".to_owned();
    assert_eq!(
        tap(&mut a, &Click::CloseRecent(files.clone())),
        vec![Effect::Close(files)]
    );
    let set = "org.ultar.settings".to_owned();
    assert_eq!(
        tap(&mut a, &Click::App(set.clone())),
        vec![Effect::Launch(set)]
    );
    assert_eq!(a.model.screen, Screen::Home);
    tap(&mut a, &Click::Recents);
    let effects = tap(&mut a, &Click::ClearRecents);
    assert_eq!(effects.len(), 1);
    assert!(a.model.recents.is_empty());
    assert!(a.built().node(&Click::ClearRecents).is_none());
    tap(&mut a, &Click::Recents);
    assert_eq!(a.model.screen, Screen::Home, "recents again = back home");
}

#[test]
fn search_finds_apps_as_one_types() {
    let mut a = home(Lang::En);
    tap(&mut a, &Click::Search);
    assert_eq!(a.model.screen, Screen::Search);
    for c in "term".chars() {
        a.handle(ultar_ui::event::Event::Key {
            key: ultar_ui::event::Key::Char(c),
            modifiers: ultar_ui::event::Modifiers::NONE,
        })
        .unwrap();
    }
    assert_eq!(a.model.query, "term");
    let field = a.built().search.unwrap();
    assert_eq!(
        a.ui().text(field).ok(),
        Some("term"),
        "the text stays after the rebuild"
    );
    let terminal = "org.ultar.terminal".to_owned();
    assert!(a.built().node(&Click::App(terminal.clone())).is_some());
    assert!(a
        .built()
        .node(&Click::App("org.ultar.files".into()))
        .is_none());
    assert_eq!(
        tap(&mut a, &Click::App(terminal.clone())),
        vec![Effect::Launch(terminal)]
    );
    assert!(a.model.query.is_empty());
    // Back from the search, and swipe up on home opens it.
    a.swipe(Point::new(180.0, 600.0), Point::new(0.0, -200.0))
        .unwrap();
    assert_eq!(a.model.screen, Screen::Search);
    tap(&mut a, &Click::Back);
    assert_eq!(a.model.screen, Screen::Home);
}

#[test]
fn the_navigation_bar_works_on_every_open_screen() {
    for screen in [Screen::Home, Screen::Quick, Screen::Recents, Screen::Search] {
        let mut m = sample(false);
        m.go(screen);
        m.page = 1;
        let mut a = app(Lang::Ar, ThemeKind::Light, m);
        tap(&mut a, &Click::Home);
        assert_eq!(
            (a.model.screen, a.model.page),
            (Screen::Home, 0),
            "{screen:?}"
        );
        let mut m = sample(false);
        m.go(screen);
        let mut a = app(Lang::Ar, ThemeKind::Light, m);
        tap(&mut a, &Click::Back);
        assert_eq!(a.model.screen, Screen::Home, "{screen:?}");
    }
}

#[test]
fn every_node_of_every_screen_can_be_tapped() {
    for lang in [Lang::Ar, Lang::En] {
        for screen in [
            Screen::Lock,
            Screen::Home,
            Screen::Quick,
            Screen::Recents,
            Screen::Search,
        ] {
            let clicks: Vec<Click> = {
                let mut m = sample(screen == Screen::Lock);
                m.go(screen);
                app(lang, ThemeKind::Dark, m)
                    .built()
                    .clicks
                    .iter()
                    .map(|(_, c)| c.clone())
                    .collect()
            };
            assert!(!clicks.is_empty());
            for click in clicks {
                let mut m = sample(screen == Screen::Lock);
                m.go(screen);
                let mut a = app(lang, ThemeKind::Dark, m);
                let before = a.model.clone();
                let effects = tap(&mut a, &click);
                // Already there: home and back on the home screen, the shown page's dot.
                let idle = screen == Screen::Home
                    && matches!(click, Click::Home | Click::Back | Click::Page(0));
                assert!(
                    idle || !effects.is_empty() || a.model != before,
                    "{lang:?} {screen:?}: {click:?} did nothing"
                );
            }
        }
    }
}
