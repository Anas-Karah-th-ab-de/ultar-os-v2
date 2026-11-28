//! The system display on the desktop background: the card in the top corner opposite the
//! icons, the values in the language's form, a dash where the device does not measure, and
//! the picture in both themes and both languages.

mod common;

use common::shots::*;
use common::*;
use ultar_desktop::model::metrics::{Gauge, Metrics};
use ultar_desktop::model::prefs::Corner;
use ultar_desktop::shell::Outcome;
use ultar_desktop::view::{metrics, Target};
use ultar_i18n::Lang;
use ultar_ui::geom::Size;
use ultar_ui::tokens::ThemeKind;

/// A minute of a working machine: values and their history.
fn busy() -> Metrics {
    let wave = |base: u8, span: u8| -> Vec<u8> {
        (0..30u8)
            .map(|i| base + (i.wrapping_mul(37) % span))
            .collect()
    };
    Metrics {
        cpu: Some(23),
        ram: Some((5_583_457_484, 16_746_053_632)),
        gpu: None,
        net: Some((1_258_291, 87_040)),
        history: [wave(8, 30), wave(30, 6), Vec::new(), wave(10, 90)],
        upload: wave(2, 12),
    }
}

fn harness(lang: Lang, theme: ThemeKind) -> Harness {
    let mut h = Harness::new(lang, theme);
    h.shell.d.places.metrics = busy();
    h.apply(Outcome::surfaces());
    h
}

fn text(v: &ultar_desktop::view::View, gauge: Gauge) -> String {
    let node = v
        .node(&Target::MetricValue(gauge))
        .expect("the value is shown");
    v.ui.text(node).map(str::to_owned).unwrap_or_default()
}

#[test]
fn the_values_read_in_the_language_and_a_dash_stands_for_what_is_not_measured() {
    let h = harness(Lang::En, ThemeKind::Dark);
    let tr = &h.shell.tr;
    let m = busy();
    let plain = |t: String| t.replace(['\u{2068}', '\u{2069}'], "");
    let shown = |g| plain(metrics::value(&m, g, tr).resolve(tr).into_owned());
    assert_eq!(shown(Gauge::Cpu), "23%");
    assert_eq!(shown(Gauge::Ram), "5.1 GB of 15.5 GB");
    assert_eq!(shown(Gauge::Net), "↓ 1.1 MB/s  ↑ 85.0 KB/s");
    assert_eq!(shown(Gauge::Gpu), "–", "no GPU load on this device");
    let de = harness(Lang::De, ThemeKind::Dark);
    let tr = &de.shell.tr;
    assert_eq!(
        plain(metrics::value(&m, Gauge::Ram, tr).resolve(tr).into_owned()),
        "5,1 GB von 15,5 GB"
    );
    let v = h.background();
    assert!(!text(&v, Gauge::Cpu).is_empty() || v.node(&Target::MetricGraph(Gauge::Cpu)).is_some());
}

#[test]
fn the_card_stands_in_the_top_corner_opposite_the_icons() {
    let mut h = harness(Lang::En, ThemeKind::Dark);
    let card = |h: &Harness| {
        let v = h.background();
        let node = v.node(&Target::MetricGraph(Gauge::Cpu)).unwrap();
        v.ui.rect(node).unwrap()
    };
    let r = card(&h);
    assert!(
        r.x > SCREEN.w / 2.0 && r.right() < SCREEN.w - 24.0,
        "top end: {r:?}"
    );
    assert!(r.y < SCREEN.h / 3.0);
    h.shell.d.prefs.desk.corner = Corner::TopEnd;
    let r = card(&h);
    assert!(
        r.right() < SCREEN.w / 2.0,
        "icons at the end: the card at the start {r:?}"
    );
    let ar = harness(Lang::Ar, ThemeKind::Dark);
    let r = card(&ar);
    assert!(
        r.right() < SCREEN.w / 2.0,
        "Arabic: the end is on the left {r:?}"
    );
}

#[test]
fn the_system_display_matches_its_references() {
    let mut problems = Vec::new();
    for (lang, theme) in VARIANTS {
        let h = harness(lang, theme);
        let mut img = blank_of(theme, SCREEN);
        let mut bg = h.background();
        over(&mut img, &mut bg, SCREEN, (0, 0));
        let mut bar = ultar_desktop::shell::build_bar(&h.shell, h.look, SCREEN.w).unwrap();
        let bar_top = (SCREEN.h - 56.0) as i32;
        over(&mut img, &mut bar, Size::new(SCREEN.w, 56.0), (0, bar_top));
        check("desk-metrics", lang, theme, &img, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
