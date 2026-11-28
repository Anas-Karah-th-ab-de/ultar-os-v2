//! Flows through the real input path: typing, formatting, saving, the unsaved-changes question,
//! find and replace, clipboard, the keyboard in Arabic and the assistant's actions.

mod common;

use common::*;
use ultar_doc::action::{AgentAction, Out};
use ultar_doc::commands::Cmd;
use ultar_doc::dialogs::{Ask, Purpose};
use ultar_doc::state::{Cursor, Doc};
use ultar_office::doc::{Align, Block, Style};
use ultar_ui::event::Key;

fn para(app: &ultar_doc::App, i: usize) -> ultar_office::doc::Para {
    match &app.doc.document.blocks[i] {
        Block::Para(p) => p.clone(),
        _ => panic!("not a paragraph"),
    }
}

#[test]
fn type_format_save_as_and_reopen() {
    let areas = areas("save");
    let mut app = app(&areas);
    type_text(&mut app, "Hello world");
    chord(&mut app, 'a');
    chord(&mut app, 'b');
    assert!(app.doc.dirty());
    assert!(
        app.title().starts_with('\u{2022}'),
        "an unsaved dot in the title"
    );
    chord(&mut app, 's');
    assert!(
        matches!(
            app.ask,
            Some(Ask::Name {
                purpose: Purpose::Save,
                ..
            })
        ),
        "a new document asks for a name"
    );
    answer_name(&mut app, "letter");
    assert!(app.ask.is_none());
    let file = areas.join("documents/letter.docx");
    assert!(file.exists());
    assert!(!app.doc.dirty());
    let again = Doc::open(&file).unwrap();
    assert_eq!(again.item_text(0, None).as_deref(), Some("Hello world"));
    let Block::Para(p) = &again.document.blocks[0] else {
        panic!("para");
    };
    assert!(p.runs.iter().all(|r| r.bold), "bold survives the file");
    // Saving again goes to the same file without a question.
    press(&mut app, Key::End);
    type_text(&mut app, "!");
    chord(&mut app, 's');
    assert!(app.ask.is_none() && !app.doc.dirty());
    assert_eq!(
        Doc::open(&file).unwrap().item_text(0, None).as_deref(),
        Some("Hello world!")
    );
    // A name that exists is refused.
    let mut other = common::app(&areas);
    type_text(&mut other, "x");
    chord(&mut other, 's');
    answer_name(&mut other, "letter");
    assert!(
        other.doc.path.is_none(),
        "an existing file is not overwritten"
    );
    assert!(other.message.is_some());
}

#[test]
fn the_toolbar_formats_the_selection() {
    let areas = areas("toolbar");
    let mut app = app(&areas);
    type_text(&mut app, "title\nbody text");
    app.doc.set_pos(
        Cursor {
            block: 0,
            cell: None,
            ch: 0,
        },
        false,
    );
    press(&mut app, Key::Home);
    key(&mut app, Key::End, shift());
    for cmd in [
        Cmd::H1,
        Cmd::AlignCenter,
        Cmd::Bold,
        Cmd::SizeUp,
        Cmd::Spacing,
    ] {
        let n = app.command_node(cmd).unwrap();
        click(&mut app, n);
    }
    let p = para(&app, 0);
    assert_eq!(p.style, Style::H1);
    assert_eq!(p.align, Align::Center);
    assert_eq!(p.line_spacing, Some(1.15));
    assert!(p.runs.iter().all(|r| r.bold));
    assert_eq!(para(&app, 1).style, Style::Normal);
    // The button shows the state at the caret.
    assert!(app.doc.is_on(Cmd::Bold) && app.doc.is_on(Cmd::H1));
    let undo = app.command_node(Cmd::Undo).unwrap();
    click(&mut app, undo);
    assert_eq!(para(&app, 0).line_spacing, None);
}

#[test]
fn the_unsaved_changes_question_on_close() {
    let areas = areas("close");
    let mut app = app(&areas);
    app.close_requested().unwrap();
    assert_eq!(
        app.take_out(),
        vec![Out::Close],
        "a clean document closes at once"
    );
    let mut app = common::app(&areas);
    type_text(&mut app, "keep me");
    app.close_requested().unwrap();
    assert!(matches!(app.ask, Some(Ask::Unsaved { then_close: true })));
    assert!(!app.take_out().contains(&Out::Close));
    {
        let n = app.nodes.cancel.unwrap();
        click(&mut app, n);
    }
    assert!(app.ask.is_none() && app.doc.dirty());
    assert!(
        !app.take_out().contains(&Out::Close),
        "cancel keeps the window open"
    );
    app.close_requested().unwrap();
    {
        let n = app.nodes.discard.unwrap();
        click(&mut app, n);
    }
    assert!(app.take_out().contains(&Out::Close));
    // Save and close: asks for the name, saves, closes.
    let mut app = common::app(&areas);
    type_text(&mut app, "saved on close");
    app.close_requested().unwrap();
    {
        let n = app.nodes.ok.unwrap();
        click(&mut app, n);
    }
    assert!(matches!(
        app.ask,
        Some(Ask::Name {
            then_close: true,
            ..
        })
    ));
    answer_name(&mut app, "closing");
    assert!(areas.join("documents/closing.docx").exists());
    assert!(app.take_out().contains(&Out::Close));
}

#[test]
fn autosave_copy_and_recovery_after_a_crash() {
    let areas = areas("crash");
    let file = areas.join("documents/crash.docx");
    let mut app = app(&areas);
    type_text(&mut app, "before");
    chord(&mut app, 's');
    answer_name(&mut app, "crash");
    type_text(&mut app, " and after");
    std::thread::sleep(std::time::Duration::from_millis(30));
    app.tick(std::time::Duration::from_secs(100));
    let copy = areas.join("documents/.crash.docx.autosave.docx");
    assert!(copy.exists(), "a changed document is copied");
    drop(app); // the "crash": nothing saved, no cleanup
    assert_eq!(
        Doc::open(&file).unwrap().item_text(0, None).as_deref(),
        Some("before")
    );
    let doc = Doc::open(&file).unwrap();
    let mut again = ultar_doc::App::new(
        ui(
            ultar_i18n::Lang::En,
            ultar_ui::tokens::ThemeKind::Dark,
            (1280.0, 800.0),
        ),
        doc,
        areas.clone(),
        None,
    )
    .unwrap();
    assert!(matches!(again.ask, Some(Ask::Recover { .. })));
    {
        let n = again.nodes.ok.unwrap();
        click(&mut again, n);
    }
    assert_eq!(text(&again, 0), "before and after");
    assert!(again.doc.dirty() && !copy.exists());
    // Discarding drops the copy and keeps the file.
    let mut app = common::app(&areas);
    type_text(&mut app, "x");
    app.tick(std::time::Duration::from_secs(100));
    assert!(areas.join("documents/.untitled.autosave.docx").exists());
    drop(app);
    let mut again = common::app_with(
        &areas,
        Doc::new(),
        ultar_i18n::Lang::En,
        ultar_ui::tokens::ThemeKind::Dark,
        (1280.0, 800.0),
    );
    assert!(
        matches!(again.ask, Some(Ask::Recover { .. })),
        "an unnamed document is offered too"
    );
    {
        let n = again.nodes.discard.unwrap();
        click(&mut again, n);
    }
    assert!(!areas.join("documents/.untitled.autosave.docx").exists());
    assert_eq!(text(&again, 0), "");
}

#[test]
fn find_replace_with_arabic_folding_and_one_undo() {
    let areas = areas("find");
    let mut app = app(&areas);
    type_text(&mut app, "one two one\nأحمد ذهب");
    chord(&mut app, 'f');
    assert_eq!(app.ask, Some(Ask::Find));
    app.ui
        .set_text(app.nodes.find_field.unwrap(), "ONE")
        .unwrap();
    app.ui
        .set_text(app.nodes.replace_field.unwrap(), "three")
        .unwrap();
    {
        let n = app.nodes.ok.unwrap();
        click(&mut app, n);
    }
    assert_eq!(app.find.hits.len(), 2, "case-insensitive by default");
    assert_eq!(app.doc.selected_text(), "one");
    {
        let n = app.nodes.replace_all.unwrap();
        click(&mut app, n);
    }
    assert_eq!(text(&app, 0), "three two three");
    app.ui
        .set_text(app.nodes.find_field.unwrap(), "احمد")
        .unwrap();
    {
        let n = app.nodes.ok.unwrap();
        click(&mut app, n);
    }
    assert_eq!(app.find.hits.len(), 1, "alef variants match");
    {
        let n = app.nodes.cancel.unwrap();
        click(&mut app, n);
    }
    assert!(app.ask.is_none());
    chord(&mut app, 'z');
    assert_eq!(text(&app, 0), "one two one", "replace all is one undo step");
}

#[test]
fn clipboard_keeps_formatting_inside_and_plain_text_outside() {
    let areas = areas("clip");
    let mut app = app(&areas);
    type_text(&mut app, "bold plain");
    app.doc.set_pos(
        Cursor {
            block: 0,
            cell: None,
            ch: 0,
        },
        false,
    );
    for _ in 0..4 {
        key(&mut app, Key::ArrowRight, shift());
    }
    chord(&mut app, 'b');
    chord(&mut app, 'c');
    let out = app.take_out();
    assert!(out.contains(&Out::Copy("bold".into())));
    key(
        &mut app,
        Key::End,
        ultar_ui::event::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    );
    chord(&mut app, 'v');
    assert!(app.take_out().contains(&Out::Paste));
    app.pasted("bold").unwrap();
    assert_eq!(text(&app, 0), "bold plainbold");
    assert!(
        para(&app, 0).runs.last().is_some_and(|r| r.bold),
        "our own copy keeps its format"
    );
    app.pasted("\u{e9}x\nnew").unwrap();
    assert_eq!(
        app.doc.document.blocks.len(),
        2,
        "plain text from outside, with a newline, splits"
    );
    chord(&mut app, 'a');
    chord(&mut app, 'x');
    assert_eq!(text(&app, 0), "");
}

#[test]
fn arabic_arrows_are_physical_and_words_move_with_ctrl() {
    let areas = areas("arrows");
    let mut app = app(&areas);
    type_text(
        &mut app,
        "\u{633}\u{644}\u{627}\u{645} \u{639}\u{644}\u{64a}\u{643}\u{645}",
    );
    app.doc.set_dir(ultar_office::doc::Dir::Rtl);
    press(&mut app, Key::Home);
    assert_eq!(app.doc.cursor.ch, 0);
    press(&mut app, Key::ArrowLeft);
    assert_eq!(
        app.doc.cursor.ch, 1,
        "left moves on in a right-to-left line"
    );
    press(&mut app, Key::ArrowRight);
    assert_eq!(app.doc.cursor.ch, 0);
    key(&mut app, Key::ArrowLeft, ctrl());
    assert!(
        app.doc.cursor.ch >= 4,
        "ctrl+left jumps a word on: {}",
        app.doc.cursor.ch
    );
    key(&mut app, Key::ArrowLeft, shift());
    assert!(app.doc.selection().is_some(), "shift extends the selection");
    press(&mut app, Key::Escape);
    assert!(app.doc.selection().is_none());
}

#[test]
fn up_and_down_follow_wrapped_lines_and_keep_the_column() {
    let areas = areas("updown");
    let mut app = app_with(
        &areas,
        Doc::new(),
        ultar_i18n::Lang::En,
        ultar_ui::tokens::ThemeKind::Dark,
        (420.0, 700.0),
    );
    type_text(&mut app, &"word ".repeat(60));
    press(&mut app, Key::Home);
    let line_start = app.doc.cursor.ch;
    key(&mut app, Key::Home, ctrl());
    key(&mut app, Key::ArrowRight, ctrl());
    let col = app.doc.cursor.ch;
    press(&mut app, Key::ArrowDown);
    assert!(
        app.doc.cursor.ch > col + 5,
        "down goes to the next wrapped line: {} -> {}",
        col,
        app.doc.cursor.ch
    );
    press(&mut app, Key::ArrowUp);
    assert!(
        app.doc.cursor.ch.abs_diff(col) <= 2,
        "and back to the same column"
    );
    press(&mut app, Key::End);
    assert!(
        app.doc.cursor.ch > col && app.doc.cursor.ch < 60 * 5,
        "End stops at the end of the line, not the paragraph"
    );
    assert_eq!(line_start, line_start);
    press(&mut app, Key::PageDown);
    assert!(app.doc.cursor.ch > 0);
}

#[test]
fn the_assistant_reads_freely_and_edits_only_after_confirmation() {
    let areas = areas("agent");
    let mut app = app(&areas);
    type_text(&mut app, "Hello world");
    let read = app
        .agent(AgentAction::ReadText { max: 5 })
        .unwrap()
        .unwrap();
    assert!(
        read.0.starts_with("Hello"),
        "read_text answers at once: {read:?}"
    );
    assert!(app.ask.is_none() && text(&app, 0) == "Hello world");
    // insert_text asks first.
    assert!(app
        .agent(AgentAction::InsertText {
            text: " there".into()
        })
        .unwrap()
        .is_none());
    assert!(matches!(app.ask, Some(Ask::Agent(_))));
    assert_eq!(
        text(&app, 0),
        "Hello world",
        "nothing changed before the person agrees"
    );
    click_ok(&mut app);
    assert_eq!(text(&app, 0), "Hello world there");
    assert!(app.take_agent_results()[0].0.starts_with("ok"));
    // replace: declined.
    app.agent(AgentAction::Replace {
        find: "Hello".into(),
        repl: "Bye".into(),
    })
    .unwrap();
    click_cancel(&mut app);
    assert_eq!(text(&app, 0), "Hello world there");
    assert!(app.take_agent_results()[0].0.starts_with("declined"));
    // replace: allowed, as one undo step.
    app.agent(AgentAction::Replace {
        find: "hello".into(),
        repl: "Bye".into(),
    })
    .unwrap();
    click_ok(&mut app);
    assert_eq!(text(&app, 0), "Bye world there");
    chord(&mut app, 'z');
    assert_eq!(text(&app, 0), "Hello world there");
    // open refuses while there are unsaved changes and opens files otherwise.
    let r = agent_open(&mut app, "files://documents/none.docx");
    assert!(r.starts_with("refused"), "{r:?}");
    let mut clean = common::app(&areas);
    let r = agent_open(&mut clean, "files://documents/none.docx");
    assert!(r.starts_with("error"), "{r:?}");
}

/// The answer of the assistant's `open`.
fn agent_open(app: &mut ultar_doc::App, uri: &str) -> String {
    app.agent(AgentAction::Open { uri: uri.into() })
        .unwrap()
        .unwrap()
        .0
}

#[test]
fn txt_opens_odt_is_refused_and_pdf_exports() {
    let areas = areas("files");
    std::fs::write(
        areas.join("documents/note.txt"),
        "plain \u{645}\u{631}\u{62d}\u{628}\u{627}\nline two",
    )
    .unwrap();
    std::fs::write(areas.join("documents/old.odt"), b"PK").unwrap();
    let (doc, err) = ultar_doc::App::open(Some("files://documents/note.txt"), &areas);
    assert!(err.is_none() && doc.document.blocks.len() == 2);
    let (_, err) = ultar_doc::App::open(Some("files://documents/old.odt"), &areas);
    assert_eq!(
        err.map(|e| e.key()),
        Some("doc.reason.unsupported"),
        "odt is said to be unsupported"
    );
    let mut app = app_with(
        &areas,
        doc,
        ultar_i18n::Lang::En,
        ultar_ui::tokens::ThemeKind::Dark,
        (1280.0, 800.0),
    );
    chord(&mut app, 's');
    assert!(
        matches!(app.ask, Some(Ask::Name { .. })),
        "a txt document is saved as .docx under a name"
    );
    {
        let n = app.nodes.cancel.unwrap();
        click(&mut app, n);
    }
    {
        let n = app.command_node(Cmd::Pdf).unwrap();
        click(&mut app, n);
    }
    answer_name(&mut app, "out");
    let pdf = std::fs::read(areas.join("documents/out.pdf")).unwrap();
    let doc = ultar_office::pdf::PdfDoc::open(pdf).unwrap();
    assert!(doc.page_text(0).contains("line two"));
}
