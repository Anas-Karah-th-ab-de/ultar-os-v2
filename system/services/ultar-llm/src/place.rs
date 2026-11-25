//! Der Ordner, den die Person für ihre Modelle gewählt hat (Einstellung `assistant.model_place`).
//! ultar-llm-bus liest die Einstellung, prüft den Pfad und schreibt ihn nach [`PLACE_FILE`];
//! der Dienst liest die Datei bei jeder Modellsuche (wirkt also ohne Neustart) und prüft selbst
//! noch einmal: nur ein schlichter Pfad unter [`ROOTS`], und auch nach dem Auflösen von
//! Verknüpfungen muss der Ordner dort liegen.

use std::path::{Path, PathBuf};

/// Die Datei, die ultar-llm-bus schreibt.
pub const PLACE_FILE: &str = "/run/ultar/llmbus/model-place";
/// Die einzigen Ordner, unter denen der Modellort liegen darf.
pub const ROOTS: [&str; 3] = [
    "/data/ultar/models",
    "/data/ultar/agent",
    "/run/ultar/media",
];
/// Längster angenommener Pfad.
pub const MAX_LEN: usize = 256;

/// Der Ordner aus `file`, wenn es einen gibt und er unter einer der `roots` liegt.
pub fn read(file: &Path, roots: &[&str]) -> Option<PathBuf> {
    if std::fs::metadata(file).ok()?.len() > MAX_LEN as u64 {
        return None;
    }
    let text = std::fs::read_to_string(file).ok()?;
    let text = text.trim_end_matches('\n');
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    let path = Path::new(text);
    if !plain(text) || !below(path, roots) {
        return None;
    }
    let real = path.canonicalize().ok()?;
    (real.is_dir() && below(&real, roots)).then_some(real)
}

/// Absolute, and every part a real name (no empty part, no `.`, no `..`); one slash at the
/// end is allowed. Checked on the text: `Path` would quietly drop a `.` in the middle.
fn plain(text: &str) -> bool {
    let inner = text.strip_suffix('/').unwrap_or(text);
    inner.starts_with('/')
        && inner[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn below(path: &Path, roots: &[&str]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("llm-place-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn a_folder_below_the_roots_counts_and_nothing_else() {
        let base = scratch("roots");
        let root = base.join("models");
        let mine = root.join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(base.join("other")).unwrap();
        let roots = [root.to_str().unwrap()];
        let file = base.join("model-place");
        let says = |text: &str| {
            std::fs::write(&file, text).unwrap();
            read(&file, &roots)
        };
        assert_eq!(says(mine.to_str().unwrap()), Some(mine.clone()));
        assert_eq!(says(""), None);
        assert_eq!(says(base.join("other").to_str().unwrap()), None);
        assert_eq!(says(&format!("{}/../other", root.display())), None);
        assert_eq!(says(&format!("{}/gone", root.display())), None);
        assert_eq!(says(&format!("{}/./mine", root.display())), None);
        assert_eq!(
            says(&format!("{}/mine/", root.display())),
            Some(mine.clone())
        );
        // a link below the root that leads out of it does not count
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("other"), root.join("out")).unwrap();
            assert_eq!(says(&format!("{}/out", root.display())), None);
        }
        assert_eq!(read(&base.join("missing"), &roots), None);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
