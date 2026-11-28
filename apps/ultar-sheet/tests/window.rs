//! The sheet window headless: it paints in Arabic and English, dark and light, at three sizes
//! and in every state; opening, typing, saving, closing with changes, the formula bar.

mod common;

use common::*;
use std::path::PathBuf;
use ultar_i18n::Lang;
use ultar_sheet::app::{Out, State};
use ultar_sheet::ask::Ask;
use ultar_sheet::cmd::Cmd;
use ultar_sheet::file::{self, DocError, Format};
use ultar_ui::event::Key;
use ultar_ui::tokens::{InputMode, SizeClass, ThemeKind};

#[test]
fn paints_in_four_variants_at_three_sizes() {
    let d = dirs("variants");
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for (size_name, size) in [("compact", COMPACT), ("medium", MEDIUM), ("wide", WIDE)] {
                let mut a = app_with(
                    &d,
                    sample(lang == Lang::Ar),
                    lang,
                    theme,
                    InputMode::Pointer,
                    size,
                );
                let tag = format!("{lang:?}-{}-{size_name}", theme.name()).to_lowercase();
                let class = a.class();
                assert_eq!(
                    class,
                    match size_name {
                        "compact" => SizeClass::Compact,
                        "medium" => SizeClass::Medium,
                        _ => SizeClass::Wide,
                    }
                );
                a.core.sel.select(1, 1);
                a.refresh().unwrap();
                shot(&mut a, &format!("sheet-{tag}"));
                // a range selected and the second tool bar open on a phone
                a.core.sel.extend_to(3, 2);
                if class == SizeClass::Compact {
                    button(&mut a, Cmd::Tools);
                }
                shot(&mut a, &format!("sheet-range-{tag}"));
            }
        }
    }
}

#[test]
fn an_arabic_sheet_runs_from_the_right_and_an_english_one_from_the_left() {
    let d = dirs("rtl");
    for rtl in [true, false] {
        let mut a = app_with(
            &d,
            sample(rtl),
            Lang::Ar,
            ThemeKind::Dark,
            InputMode::Pointer,
            WIDE,
        );
        let a1 = cell_pos(&mut a, 1, 0);
        let b1 = cell_pos(&mut a, 1, 1);
        assert_eq!(
            a1.x > b1.x,
            rtl,
            "column A is at the {} edge",
            if rtl { "right" } else { "left" }
        );
        // a click on column A selects it wherever it is
        let mut a2 = app_with(
            &d,
            sample(rtl),
            Lang::En,
            ThemeKind::Light,
            InputMode::Pointer,
            WIDE,
        );
        let p = cell_pos(&mut a2, 2, 0);
        a2.pointer(ev_down(p)).unwrap();
        a2.pointer(ev_up(p)).unwrap();
        assert_eq!(a2.core.sel.active, (2, 0));
    }
}

#[test]
fn the_states_are_shown_and_have_a_way_out() {
    let d = dirs("states");
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let tag = format!("{lang:?}-{}", theme.name()).to_lowercase();
            // empty: a new workbook says what to do
            let mut a = app(&d, lang, theme, WIDE);
            assert!(a.core.book.sheet().cells.is_empty());
            shot(&mut a, &format!("state-empty-{tag}"));
            // loading
            a.loading("حسابات.xlsx").unwrap();
            assert!(matches!(a.state(), State::Loading(_)));
            shot(&mut a, &format!("state-loading-{tag}"));
            // damaged
            a.loaded(PathBuf::from("x.xlsx"), Err(DocError::Damaged))
                .unwrap();
            assert_eq!(a.state(), &State::Damaged(DocError::Damaged));
            shot(&mut a, &format!("state-damaged-{tag}"));
            assert!(a.node_of(Cmd::New).is_some(), "a way out");
            // too large
            a.loaded(PathBuf::from("x.xlsx"), Err(DocError::TooBig))
                .unwrap();
            assert_eq!(a.state(), &State::TooBig);
            shot(&mut a, &format!("state-toobig-{tag}"));
            // "New" leaves the state
            button(&mut a, Cmd::New);
            assert_eq!(a.state(), &State::Ready);
        }
    }
    // compact too
    let mut a = app(&d, Lang::Ar, ThemeKind::Dark, COMPACT);
    a.loaded(PathBuf::from("x.xlsx"), Err(DocError::Damaged))
        .unwrap();
    shot(&mut a, "state-damaged-compact");
}

#[test]
fn a_real_damaged_file_and_a_real_workbook_open_from_a_uri() {
    let d = dirs("open");
    std::fs::write(d.areas.join("documents/broken.xlsx"), b"PK not really").unwrap();
    let book = sample(true);
    file::save(
        &d.areas.join("documents/حسابات.xlsx"),
        &book.wb,
        Format::Xlsx,
        0,
    )
    .unwrap();
    let mut a = app(&d, Lang::Ar, ThemeKind::Dark, WIDE);
    outs(&mut a);
    // the workbook, at a sheet and a cell
    a.open_uri("files://documents/%D8%AD%D8%B3%D8%A7%D8%A8%D8%A7%D8%AA.xlsx#sheet=Sheet2&cell=C5")
        .unwrap();
    let load = outs(&mut a)
        .into_iter()
        .find_map(|o| if let Out::Load(p) = o { Some(p) } else { None })
        .unwrap();
    assert!(matches!(a.state(), State::Loading(_)));
    a.loaded(load.clone(), file::open(&load)).unwrap();
    assert_eq!(a.state(), &State::Ready);
    assert_eq!(a.core.book.wb.sheets.len(), 2);
    assert_eq!((a.core.book.active, a.core.sel.active), (1, (4, 2)));
    let rep = a.report();
    assert_eq!(rep.sheet, "Sheet2");
    assert_eq!(rep.cell, "C5");
    assert_eq!(
        rep.uri.as_deref(),
        Some("files://documents/%D8%AD%D8%B3%D8%A7%D8%A8%D8%A7%D8%AA.xlsx#sheet=Sheet2&cell=C5")
    );
    // formulas of the file are calculated
    a.core.show_sheet(0);
    assert_eq!(a.core.book.shown(1, 3).text, "30");
    assert_eq!(a.core.book.shown(6, 3).text, "#DIV/0!");
    // the broken one
    let broken = d.areas.join("documents/broken.xlsx");
    a.loaded(broken.clone(), file::open(&broken)).unwrap();
    assert_eq!(a.state(), &State::Damaged(DocError::Damaged));
    // a place outside the areas is refused with a message, not opened
    let mut b = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    outs(&mut b);
    b.open_uri("files://documents/../../etc/passwd").unwrap();
    assert!(!outs(&mut b).iter().any(|o| matches!(o, Out::Load(_))));
}

#[test]
fn typing_saving_and_closing_with_changes() {
    let d = dirs("save");
    let mut a = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    outs(&mut a);
    assert!(a.grid_focused());
    type_text(&mut a, "42");
    plain(&mut a, Key::Enter);
    type_text(&mut a, "=A1+8");
    plain(&mut a, Key::Enter);
    assert_eq!(a.core.book.shown(1, 0).text, "50");
    assert!(a.title().starts_with("• "));
    // Ctrl+S on a new workbook asks for a name in Documents
    ctrl(&mut a, 's');
    assert!(matches!(a.question(), Some(Ask::Name(_))));
    let [ok, _, _, field] = a.dialog_nodes();
    a.ui.set_text(field.unwrap(), "totals").unwrap();
    click(&mut a, ok.unwrap());
    assert!(a.question().is_none());
    let saved = d.areas.join("documents/totals.xlsx");
    assert!(saved.exists());
    assert!(!a.core.book.dirty());
    let back = file::open(&saved).unwrap();
    assert_eq!(back.book.sheets[0].get(1, 0), "=A1+8");
    // closing without changes closes at once
    a.close_requested().unwrap();
    assert!(outs(&mut a).contains(&Out::Close));
    // with changes it asks; "don't save" closes, cancel stays
    put(&mut a, 5, 5, "x");
    a.close_requested().unwrap();
    assert!(!outs(&mut a).contains(&Out::Close));
    assert!(matches!(a.question(), Some(Ask::Unsaved(_))));
    let [_, cancel, discard, _] = a.dialog_nodes();
    click(&mut a, cancel.unwrap());
    assert!(a.question().is_none() && a.core.book.dirty());
    a.close_requested().unwrap();
    let [ok, _, discard2, _] = a.dialog_nodes();
    let _ = (discard, discard2);
    click(&mut a, discard2.unwrap());
    assert!(outs(&mut a).contains(&Out::Close));
    // "save" in the question saves and closes
    let mut b = app(&d, Lang::Ar, ThemeKind::Light, WIDE);
    outs(&mut b);
    b.core.book.path = Some(saved.clone());
    put(&mut b, 0, 0, "7");
    b.close_requested().unwrap();
    let [ok2, ..] = b.dialog_nodes();
    let _ = ok;
    click(&mut b, ok2.unwrap());
    assert!(outs(&mut b).contains(&Out::Close));
    assert_eq!(file::open(&saved).unwrap().book.sheets[0].get(0, 0), "7");
}

#[test]
fn csv_in_windows_1256_opens_and_saves_back_in_it() {
    let d = dirs("csv");
    let (bytes, _) = file::write_bytes(
        &sample(false).wb,
        Format::Csv {
            enc: ultar_office::code::Enc::Win1256,
            sep: ';',
            eol: ultar_office::code::Eol::CrLf,
        },
        0,
    )
    .unwrap();
    let p = d.areas.join("documents/قائمة.csv");
    std::fs::write(&p, &bytes).unwrap();
    let mut a = app(&d, Lang::Ar, ThemeKind::Dark, MEDIUM);
    a.loaded(p.clone(), file::open(&p)).unwrap();
    assert_eq!(a.core.book.sheet().get(1, 0), "تفاح");
    put(&mut a, 8, 0, "جديد");
    ctrl(&mut a, 's');
    let again = std::fs::read(&p).unwrap();
    assert!(
        !again.starts_with(&[0xEF, 0xBB, 0xBF]),
        "still Windows-1256"
    );
    assert_eq!(file::open(&p).unwrap().book.sheets[0].get(8, 0), "جديد");
    // the message says a CSV keeps only values
    assert!(a.core.message.is_some());
}

#[test]
fn the_formula_bar_edits_the_active_cell() {
    let d = dirs("bar");
    let mut a = app_with(
        &d,
        sample(false),
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
    );
    a.core.sel.select(1, 1);
    a.refresh().unwrap();
    let f = a.formula_node().unwrap();
    assert_eq!(a.ui.text(f).unwrap(), "12");
    // the bar follows the selection
    a.core.sel.select(1, 3);
    a.refresh().unwrap();
    assert_eq!(a.ui.text(f).unwrap(), "=B2*C2");
}
