//! Entries of programs (`.desktop`, freedesktop.org Desktop Entry) and the fields of a
//! package (`dpkg-deb --field`): data written by others, read defensively.

/// One program entry that becomes an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The entry's file name without `.desktop` (`org.xfce.mousepad`).
    pub file: String,
    /// `Name`.
    pub name: String,
    /// `Name[ar]`, `Name[en]`, `Name[de]` where present.
    pub names: Vec<(String, String)>,
    /// The program (first word of `Exec`).
    pub exec: String,
    /// Its arguments (field codes like `%f` removed).
    pub args: Vec<String>,
    /// `Icon`.
    pub icon: String,
}

/// Longest text taken from an entry.
const MAX_TEXT: usize = 128;
/// Most arguments taken from `Exec`.
const MAX_ARGS: usize = 16;

fn clean(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty() && t.len() <= MAX_TEXT && !t.chars().any(char::is_control)).then(|| t.to_owned())
}

/// Field codes of `Exec` that stand for files, URLs or the entry itself: dropped (the
/// app is started without them).
const FIELD_CODES: [&str; 13] = [
    "%f", "%F", "%u", "%U", "%i", "%c", "%k", "%d", "%D", "%n", "%N", "%v", "%m",
];

/// Splits `Exec` into words (double quotes group, backslash escapes inside quotes) and
/// drops the field codes (`%f %F %u %U %i %c %k …`); `None` for anything odd.
pub fn exec_words(exec: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut escaped, mut started) = (false, false, false);
    for ch in exec.chars() {
        match (quoted, escaped, ch) {
            (true, true, c) => {
                word.push(c);
                escaped = false;
            }
            (true, false, '\\') => escaped = true,
            (_, false, '"') => {
                quoted = !quoted;
                started = true;
            }
            (false, false, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, false, c) => {
                word.push(c);
                started = true;
            }
            (false, true, _) => return None,
        }
    }
    if quoted || escaped {
        return None;
    }
    if started {
        words.push(word);
    }
    let words: Vec<String> = words
        .into_iter()
        .filter(|w| !FIELD_CODES.contains(&w.as_str()))
        .collect();
    let ok = !words.is_empty()
        && words.len() <= MAX_ARGS
        && words
            .iter()
            .all(|w| w.len() <= MAX_TEXT && !w.contains('\0') && !w.contains('%'));
    ok.then_some(words)
}

/// Reads the `[Desktop Entry]` group of `text`; `None` unless it is a visible
/// application with a window (`Type=Application`, not `NoDisplay`/`Hidden`, not
/// `Terminal`).
pub fn parse_entry(file: &str, text: &str) -> Option<Entry> {
    let mut in_group = false;
    let (mut name, mut names, mut exec, mut icon) = (None, Vec::new(), None, String::new());
    let mut application = false;
    for line in text.lines().take(512) {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_group) else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Type" => application = value == "Application",
            "Name" => name = clean(value),
            "Name[ar]" | "Name[en]" | "Name[de]" => {
                if let Some(v) = clean(value) {
                    names.push((key[5..7].to_owned(), v));
                }
            }
            "Exec" => exec = exec_words(value),
            "Icon" => icon = clean(value).unwrap_or_default(),
            "NoDisplay" | "Hidden" | "Terminal" if value == "true" => return None,
            _ => {}
        }
    }
    let mut words = exec?;
    let name = name?;
    let program = words.remove(0);
    (application && clean(file).is_some()).then(|| Entry {
        file: file.to_owned(),
        name,
        names,
        exec: program,
        args: words,
        icon,
    })
}

/// The fields of a package as `dpkg-deb --field <file> Package Version Maintainer`
/// prints them (`Key: value` lines).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Control {
    /// `Package`.
    pub package: String,
    /// `Version`.
    pub version: String,
    /// `Maintainer`.
    pub maintainer: String,
}

/// A Debian package name: `[a-z0-9][a-z0-9+.-]+`, at most 64 bytes.
pub fn package_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    (2..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"+.-".contains(c))
}

/// Reads the printed fields; `None` unless the package name is a valid one.
pub fn parse_control(text: &str) -> Option<Control> {
    let mut c = Control::default();
    for line in text.lines().take(64) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = clean(value).unwrap_or_default();
        match key.trim() {
            "Package" => c.package = value,
            "Version" => c.version = value,
            "Maintainer" => c.maintainer = value,
            _ => {}
        }
    }
    package_name_ok(&c.package).then_some(c)
}

/// A package version as the launcher keeps it (`1:2.3~rc1-1` → `1.2.3.rc1-1`).
pub fn launcher_version(version: &str) -> String {
    let v: String = version
        .chars()
        .map(|c| if c == ':' || c == '~' { '.' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || ".+-".contains(*c))
        .take(32)
        .collect();
    if v.is_empty() {
        "0".to_owned()
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUSEPAD: &str = "[Desktop Entry]\nVersion=1.0\nType=Application\nName=Mousepad\n\
        Name[ar]=ماوس باد\nName[de]=Mousepad\nExec=mousepad %U\nIcon=org.xfce.mousepad\n\
        Terminal=false\n[Desktop Action new]\nExec=mousepad --new\n";

    #[test]
    fn an_entry_becomes_an_app() {
        let e = parse_entry("org.xfce.mousepad", MOUSEPAD).unwrap();
        assert_eq!(e.name, "Mousepad");
        assert_eq!(e.exec, "mousepad");
        assert!(e.args.is_empty());
        assert_eq!(e.icon, "org.xfce.mousepad");
        assert!(e.names.contains(&("ar".into(), "ماوس باد".into())));
    }

    #[test]
    fn hidden_terminal_and_odd_entries_do_not() {
        for (from, to) in [
            ("Terminal=false", "Terminal=true"),
            ("Type=Application", "Type=Link"),
            ("Terminal=false", "NoDisplay=true"),
            ("Exec=mousepad %U", "Exec=\"unterminated"),
            ("Exec=mousepad %U", "Exec="),
        ] {
            assert_eq!(parse_entry("x", &MOUSEPAD.replace(from, to)), None, "{to}");
        }
    }

    #[test]
    fn exec_lines() {
        assert_eq!(
            exec_words("\"/opt/My App/run\" --x %f"),
            Some(vec!["/opt/My App/run".into(), "--x".into()])
        );
        assert_eq!(
            exec_words("sh -c \"echo \\\"hi\\\"\""),
            Some(vec!["sh".into(), "-c".into(), "echo \"hi\"".into()])
        );
        assert_eq!(exec_words("a %%"), None);
        assert_eq!(exec_words("%f"), None);
    }

    #[test]
    fn control_fields_and_versions() {
        let c = parse_control(
            "Package: mousepad\nVersion: 0.6.2-1\nMaintainer: Xubuntu <x@u.org>\n",
        )
        .unwrap();
        assert_eq!(c.package, "mousepad");
        assert_eq!(c.maintainer, "Xubuntu <x@u.org>");
        assert_eq!(parse_control("Package: ../etc\n"), None);
        assert_eq!(parse_control("Version: 1\n"), None);
        assert_eq!(launcher_version("1:2.3~rc1-1"), "1.2.3.rc1-1");
        assert_eq!(launcher_version(""), "0");
        assert!(package_name_ok("libgtk-4-1") && package_name_ok("g++"));
        assert!(!package_name_ok("Mouse") && !package_name_ok("a") && !package_name_ok("-x"));
    }
}
