use super::*;
use ultar_i18n::Lang;

fn tr(lang: Lang) -> Translator {
    crate::i18n::translator(lang)
}

fn home(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ultar-term-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_default();
    d
}

fn run(sh: &mut Shell, line: &str, lang: Lang) -> (Vec<String>, Next) {
    let mut out = Vec::new();
    let next = sh.run(line, &tr(lang), &mut out);
    (out, next)
}

#[test]
fn words_keep_quotes_and_escapes() {
    assert_eq!(
        words("ls  -r 'a b' \"ج د\" x\\ y").unwrap_or_default(),
        ["ls", "-r", "a b", "ج د", "x y"]
    );
    assert_eq!(words("echo ''").unwrap_or_default(), ["echo", ""]);
    assert_eq!(words("echo \"open"), None);
}

#[test]
fn arabic_and_english_names_are_the_same_command() {
    let names = Names::load();
    assert_eq!(names.find("اعرض"), Some(Builtin::Ls));
    assert_eq!(names.find("LS"), Some(Builtin::Ls));
    assert_eq!(names.find("انتقل"), names.find("cd"));
    assert_eq!(names.find("خروج"), Some(Builtin::Exit));
    assert_eq!(names.find("nothing"), None);
}

#[test]
fn files_and_folders_in_arabic() {
    let h = home("files");
    let mut sh = Shell::new(h.clone());
    assert_eq!(sh.prompt(), "~ $");
    run(&mut sh, "أنشئ مشاريع", Lang::Ar);
    run(&mut sh, "انتقل مشاريع", Lang::Ar);
    assert_eq!(sh.cwd, h.join("مشاريع"));
    assert_eq!(sh.prompt(), "~/مشاريع $");
    std::fs::write(sh.cwd.join("ملاحظة.txt"), "سطر أول\nline two\n").unwrap_or_default();
    let (out, _) = run(&mut sh, "اقرأ ملاحظة.txt", Lang::Ar);
    assert_eq!(out, ["سطر أول", "line two"]);
    run(&mut sh, "انسخ ملاحظة.txt نسخة.txt", Lang::Ar);
    run(&mut sh, "انقل نسخة.txt ثانية.txt", Lang::Ar);
    let (out, _) = run(&mut sh, "اعرض", Lang::Ar);
    assert_eq!(out, ["ثانية.txt", "ملاحظة.txt"]);
    run(&mut sh, "cd ..", Lang::En);
    let (out, _) = run(&mut sh, "rm مشاريع", Lang::Ar);
    assert!(out[0].contains("مجلد"), "{out:?}");
    run(&mut sh, "احذف -r مشاريع", Lang::Ar);
    let (out, _) = run(&mut sh, "ls", Lang::En);
    assert_eq!(out, ["(empty)"]);
    let (out, _) = run(&mut sh, "اقرأ غير-موجود", Lang::Ar);
    assert!(out[0].starts_with("غير موجود"), "{out:?}");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn unknown_commands_programs_clear_and_exit() {
    let h = home("next");
    let mut sh = Shell::new(h.clone());
    let (out, next) = run(&mut sh, "لاشيء", Lang::Ar);
    assert_eq!(next, Next::Done);
    assert!(out[0].contains("«لاشيء»"));
    sh.path = vec![PathBuf::from("/bin"), PathBuf::from("/usr/bin")];
    let (_, next) = run(&mut sh, "sh -c true", Lang::En);
    assert!(matches!(next, Next::Program(_, ref a) if a == &["-c", "true"]));
    assert_eq!(run(&mut sh, "امسح", Lang::Ar).1, Next::Clear);
    assert_eq!(run(&mut sh, "exit", Lang::En).1, Next::Exit);
    let (out, _) = run(&mut sh, "السجل", Lang::Ar);
    assert_eq!(out.len(), 5, "the history command itself is in it");
    let (out, _) = run(&mut sh, "help", Lang::En);
    assert!(out.len() > 15);
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn paths_are_resolved_in_the_text() {
    let sh = Shell::new(PathBuf::from("/areas/home"));
    assert_eq!(
        sh.resolve("../documents/a"),
        PathBuf::from("/areas/documents/a")
    );
    assert_eq!(sh.resolve("~/x"), PathBuf::from("/areas/home/x"));
    assert_eq!(sh.resolve("/../../etc"), PathBuf::from("/etc"));
}

#[test]
fn dates() {
    assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
    assert_eq!(utc(1_795_623_663), "2026-11-25 16:21:03 UTC");
    assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 UTC");
}
