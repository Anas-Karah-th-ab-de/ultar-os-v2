//! The shell: splits a command line into words, runs the built-in commands (each with an
//! Arabic and an English name) and names the program for anything else.

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use ultar_i18n::{Arg, Translator};

/// Most lines of a file `cat` shows.
pub const MAX_CAT_LINES: usize = 2000;
/// Most commands kept in the history.
pub const MAX_HISTORY: usize = 500;

/// A built-in command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Help,
    Ls,
    Cd,
    Pwd,
    Cat,
    Echo,
    Mkdir,
    Rm,
    Cp,
    Mv,
    Clear,
    Date,
    Whoami,
    Uname,
    History,
    Env,
    Exit,
}

/// Each built-in with its Arabic and English names.
const NAMES: [(&str, &str, Builtin); 17] = [
    ("مساعدة", "help", Builtin::Help),
    ("اعرض", "ls", Builtin::Ls),
    ("انتقل", "cd", Builtin::Cd),
    ("أين", "pwd", Builtin::Pwd),
    ("اقرأ", "cat", Builtin::Cat),
    ("اطبع", "echo", Builtin::Echo),
    ("أنشئ", "mkdir", Builtin::Mkdir),
    ("احذف", "rm", Builtin::Rm),
    ("انسخ", "cp", Builtin::Cp),
    ("انقل", "mv", Builtin::Mv),
    ("امسح", "clear", Builtin::Clear),
    ("التاريخ", "date", Builtin::Date),
    ("من", "whoami", Builtin::Whoami),
    ("النظام", "uname", Builtin::Uname),
    ("السجل", "history", Builtin::History),
    ("بيئة", "env", Builtin::Env),
    ("خروج", "exit", Builtin::Exit),
];

/// The built-in called `name` (Arabic or English; English in any case).
pub fn builtin(name: &str) -> Option<Builtin> {
    let lower = name.to_lowercase();
    NAMES
        .iter()
        .find(|(ar, en, _)| *ar == name || *en == lower)
        .map(|(_, _, b)| *b)
}

/// Splits a command line into words: spaces separate, "…" and '…' keep spaces, a backslash
/// keeps the next character. `None` for an open quote.
pub fn words(line: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                has = true;
            }
            (None, '\\') => {
                cur.push(chars.next()?);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}

/// What a command line asks the terminal to do besides printing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Nothing more.
    Done,
    /// Clear the screen.
    Clear,
    /// Close the terminal.
    Exit,
    /// Run this program with these arguments in the current folder.
    Program(PathBuf, Vec<String>),
}

/// The state of the shell: current folder, home, history.
pub struct Shell {
    /// The current folder.
    pub cwd: PathBuf,
    /// Where `~` and a bare `cd` go.
    pub home: PathBuf,
    /// Earlier command lines, oldest first.
    pub history: Vec<String>,
    /// Where programs are looked for.
    pub path: Vec<PathBuf>,
}

impl Shell {
    /// A shell starting in `home`.
    pub fn new(home: PathBuf) -> Shell {
        let path = std::env::var("PATH")
            .unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into())
            .split(':')
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
        Shell {
            cwd: home.clone(),
            home,
            history: Vec::new(),
            path,
        }
    }

    /// The prompt: the current folder, `~` for home.
    pub fn prompt(&self) -> String {
        let shown = match self.cwd.strip_prefix(&self.home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => self.cwd.display().to_string(),
        };
        format!("{shown} $")
    }

    /// `arg` as a path: `~` is home, relative paths start at the current folder, `.` and
    /// `..` are resolved in the text (not through links).
    pub fn resolve(&self, arg: &str) -> PathBuf {
        let raw = if arg == "~" {
            self.home.clone()
        } else if let Some(rest) = arg.strip_prefix("~/") {
            self.home.join(rest)
        } else {
            self.cwd.join(arg)
        };
        let mut out = PathBuf::from("/");
        for c in raw.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::Normal(n) => out.push(n),
                Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
            }
        }
        out
    }

    /// Runs one command line; printed lines go to `out` (translated with `tr`).
    pub fn run(&mut self, line: &str, tr: &Translator, out: &mut Vec<String>) -> Next {
        let line = line.trim();
        if line.is_empty() {
            return Next::Done;
        }
        if self.history.last().map(String::as_str) != Some(line) {
            self.history.push(line.to_owned());
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
        }
        let Some(words) = words(line) else {
            out.push(tr.t_args("term.usage", &[("usage", Arg::Str("\"…\""))]));
            return Next::Done;
        };
        let Some((name, args)) = words.split_first() else {
            return Next::Done;
        };
        match builtin(name) {
            Some(b) => self.builtin(b, args, tr, out),
            None => match self.find(name) {
                Some(program) => Next::Program(program, args.to_vec()),
                None => {
                    out.push(tr.t_args("term.unknown", &[("name", Arg::Str(name))]));
                    Next::Done
                }
            },
        }
    }

    /// The program called `name`: a path with a `/`, else the first executable in [`Shell::path`].
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        let executable = |p: &Path| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        };
        if name.contains('/') {
            let p = self.resolve(name);
            return executable(&p).then_some(p);
        }
        self.path
            .iter()
            .map(|d| d.join(name))
            .find(|p| executable(p))
    }

    fn builtin(
        &mut self,
        b: Builtin,
        args: &[String],
        tr: &Translator,
        out: &mut Vec<String>,
    ) -> Next {
        let say = |key: &str, path: &Path| {
            tr.t_args(key, &[("path", Arg::Str(&path.display().to_string()))])
        };
        match b {
            Builtin::Help => {
                for key in [
                    "term.help.title",
                    "term.help.ls",
                    "term.help.cd",
                    "term.help.pwd",
                    "term.help.cat",
                    "term.help.echo",
                    "term.help.mkdir",
                    "term.help.rm",
                    "term.help.cp",
                    "term.help.mv",
                    "term.help.clear",
                    "term.help.date",
                    "term.help.whoami",
                    "term.help.uname",
                    "term.help.history",
                    "term.help.env",
                    "term.help.exit",
                    "term.help.programs",
                ] {
                    out.push(tr.t(key).into_owned());
                }
            }
            Builtin::Ls => {
                let dir = args
                    .first()
                    .map_or_else(|| self.cwd.clone(), |a| self.resolve(a));
                match std::fs::read_dir(&dir) {
                    Ok(entries) => {
                        let mut names: Vec<String> = entries
                            .filter_map(Result::ok)
                            .map(|e| {
                                let name = e.file_name().to_string_lossy().into_owned();
                                if e.file_type().is_ok_and(|t| t.is_dir()) {
                                    format!("{name}/")
                                } else {
                                    name
                                }
                            })
                            .collect();
                        names.sort_by_key(|n| n.to_lowercase());
                        if names.is_empty() {
                            out.push(tr.t("term.empty").into_owned());
                        }
                        out.extend(names);
                    }
                    Err(e) => out.push(io_message(tr, &dir, &e)),
                }
            }
            Builtin::Cd => {
                let to = args
                    .first()
                    .map_or_else(|| self.home.clone(), |a| self.resolve(a));
                match std::fs::metadata(&to) {
                    Ok(m) if m.is_dir() => self.cwd = to,
                    Ok(_) => out.push(say("term.not_dir", &to)),
                    Err(e) => out.push(io_message(tr, &to, &e)),
                }
            }
            Builtin::Pwd => out.push(self.cwd.display().to_string()),
            Builtin::Cat => {
                if args.is_empty() {
                    out.push(usage(tr, "cat <file>"));
                }
                for a in args {
                    let p = self.resolve(a);
                    match std::fs::read(&p) {
                        Ok(bytes) if bytes.iter().take(4096).any(|b| *b == 0) => {
                            out.push(say("term.binary", &p))
                        }
                        Ok(bytes) => out.extend(
                            String::from_utf8_lossy(&bytes)
                                .lines()
                                .take(MAX_CAT_LINES)
                                .map(str::to_owned),
                        ),
                        Err(e) => out.push(io_message(tr, &p, &e)),
                    }
                }
            }
            Builtin::Echo => out.push(args.join(" ")),
            Builtin::Mkdir => {
                if args.is_empty() {
                    out.push(usage(tr, "mkdir <folder>"));
                }
                for a in args {
                    let p = self.resolve(a);
                    if let Err(e) = std::fs::create_dir(&p) {
                        out.push(io_message(tr, &p, &e));
                    }
                }
            }
            Builtin::Rm => {
                let recursive = args.iter().any(|a| a == "-r" || a == "-rf");
                let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
                if paths.is_empty() {
                    out.push(usage(tr, "rm [-r] <path>"));
                }
                for a in paths {
                    let p = self.resolve(a);
                    let is_dir = std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir());
                    let done = match (is_dir, recursive) {
                        (true, false) => {
                            out.push(say("term.is_dir", &p));
                            continue;
                        }
                        (true, true) => std::fs::remove_dir_all(&p),
                        (false, _) => std::fs::remove_file(&p),
                    };
                    if let Err(e) = done {
                        out.push(io_message(tr, &p, &e));
                    }
                }
            }
            Builtin::Cp | Builtin::Mv => {
                let [from, to] = args else {
                    out.push(usage(
                        tr,
                        if b == Builtin::Cp {
                            "cp <from> <to>"
                        } else {
                            "mv <from> <to>"
                        },
                    ));
                    return Next::Done;
                };
                let (from, mut to) = (self.resolve(from), self.resolve(to));
                if to.is_dir() {
                    if let Some(name) = from.file_name() {
                        to = to.join(name);
                    }
                }
                let done = if b == Builtin::Cp {
                    copy_tree(&from, &to)
                } else {
                    // a folder may only move by copy and delete (the kernel answers EXDEV)
                    std::fs::rename(&from, &to).or_else(|e| {
                        if e.raw_os_error() == Some(18) {
                            copy_tree(&from, &to).and_then(|()| remove_tree(&from))
                        } else {
                            Err(e)
                        }
                    })
                };
                if let Err(e) = done {
                    out.push(io_message(tr, &from, &e));
                }
            }
            Builtin::Clear => return Next::Clear,
            Builtin::Date => out.push(utc_now()),
            Builtin::Whoami => out.push(
                std::env::var("USER")
                    .or_else(|_| std::env::var("LOGNAME"))
                    .unwrap_or_else(|_| "ultar".into()),
            ),
            Builtin::Uname => out.push(os_name()),
            Builtin::History => out.extend(
                self.history
                    .iter()
                    .enumerate()
                    .map(|(i, h)| format!("{:>4}  {h}", i + 1)),
            ),
            Builtin::Env => {
                let mut vars: Vec<String> =
                    std::env::vars().map(|(k, v)| format!("{k}={v}")).collect();
                vars.sort();
                out.extend(vars);
            }
            Builtin::Exit => return Next::Exit,
        }
        Next::Done
    }
}

fn usage(tr: &Translator, usage: &str) -> String {
    tr.t_args("term.usage", &[("usage", Arg::Str(usage))])
}

fn io_message(tr: &Translator, path: &Path, e: &std::io::Error) -> String {
    let shown = path.display().to_string();
    let key = match e.kind() {
        std::io::ErrorKind::NotFound => "term.missing",
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
            "term.denied"
        }
        std::io::ErrorKind::AlreadyExists => "term.exists",
        std::io::ErrorKind::NotADirectory => "term.not_dir",
        std::io::ErrorKind::IsADirectory => "term.is_dir",
        std::io::ErrorKind::DirectoryNotEmpty => "term.not_empty",
        _ => {
            return tr.t_args(
                "term.failed",
                &[
                    ("path", Arg::Str(&shown)),
                    ("reason", Arg::Str(&e.to_string())),
                ],
            )
        }
    };
    tr.t_args(key, &[("path", Arg::Str(&shown))])
}

/// Copies a file, or a folder with everything in it (links are copied as links).
pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else if meta.file_type().is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
    } else {
        if to.exists() {
            return Err(std::io::ErrorKind::AlreadyExists.into());
        }
        std::fs::copy(from, to).map(|_| ())
    }
}

fn remove_tree(p: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(p)?.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// The system's name and version from `os-release`.
fn os_name() -> String {
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| "Ultar OS".into())
}

/// The time now as `YYYY-MM-DD HH:MM:SS UTC`.
pub fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    utc(secs)
}

/// `secs` since 1970 as `YYYY-MM-DD HH:MM:SS UTC` (proleptic Gregorian calendar).
pub fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // civil_from_days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
