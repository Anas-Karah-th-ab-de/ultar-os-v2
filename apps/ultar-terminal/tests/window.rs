//! The terminal window headless: commands in Arabic, a program's output, history.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_i18n::Lang;
use ultar_terminal::app::Terminal;
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

fn term(name: &str) -> (Terminal, PathBuf) {
    let home = std::env::temp_dir().join(format!("ultar-term-win-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let ui = Ui::new(
        PaintEnv::new(Lang::Ar, ThemeKind::Dark),
        InputMode::Pointer,
        Size::new(800.0, 500.0),
        ultar_terminal::i18n::translator(Lang::Ar),
    )
    .unwrap();
    (Terminal::new(ui, home.clone()).unwrap(), home)
}

#[test]
fn arabic_commands_show_their_output_and_the_history_walks() {
    let (mut t, home) = term("cmd");
    t.submit("أنشئ مجلد").unwrap();
    t.submit("اعرض").unwrap();
    assert!(t.lines.iter().any(|l| l == "مجلد/"), "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l == "~ $ اعرض"));
    let up = Event::Key {
        key: Key::ArrowUp,
        modifiers: Modifiers::default(),
    };
    t.key(up).unwrap();
    t.submit("امسح").unwrap();
    assert!(t.lines.is_empty());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_program_runs_and_its_output_arrives() {
    let (mut t, home) = term("prog");
    t.shell.path = vec![PathBuf::from("/bin"), PathBuf::from("/usr/bin")];
    t.submit("sh -c 'echo مرحبا; exit 2'").unwrap();
    assert!(t.busy());
    let start = Instant::now();
    while t.busy() && start.elapsed() < Duration::from_secs(10) {
        t.poll().unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!t.busy());
    assert!(t.lines.iter().any(|l| l == "مرحبا"), "{:?}", t.lines);
    assert!(t.lines.last().unwrap().contains('2'));
    t.submit("خروج").unwrap();
    assert!(t.closed());
    let _ = std::fs::remove_dir_all(home);
}
