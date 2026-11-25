//! Where the person keeps their models (`assistant.model_place`). The model service has no
//! bus connection, so this service passes the place on: it checks the path and writes it to
//! a small file in its own directory, which the model service reads with every model search
//! (and checks once more, `system/services/ultar-llm/src/place.rs`). A path that fails the
//! check is never written; the file is emptied instead, so the old place no longer counts.

use std::io::Write;
use std::path::Path;
use ultar_bus::messages::settings::SettingValue;

/// The setting that names the folder.
pub const PLACE_KEY: &str = "assistant.model_place";
/// The file the model service reads (the service file gives it with `--place-file`).
pub const PLACE_FILE: &str = "/run/ultar/llmbus/model-place";
/// The only folders a place may lie below: the model folders and removable media.
pub const ROOTS: [&str; 3] = [
    "/data/ultar/models",
    "/data/ultar/agent",
    "/run/ultar/media",
];
/// The longest path accepted.
pub const MAX_LEN: usize = 256;

/// The path when it is a plain absolute path below one of [`ROOTS`] (no `.` or `..`, no
/// control characters, not too long); `None` otherwise.
pub fn checked(text: &str) -> Option<&str> {
    if text.is_empty() || text.len() > MAX_LEN || text.chars().any(char::is_control) {
        return None;
    }
    let below = ROOTS.iter().any(|root| Path::new(text).starts_with(root));
    (plain(text) && below).then_some(text)
}

/// Absolute, and every part a real name (no empty part, no `.`, no `..`); one slash at the
/// end is allowed. Checked on the text: `Path` would quietly drop a `.` in the middle.
pub fn plain(text: &str) -> bool {
    let inner = text.strip_suffix('/').unwrap_or(text);
    inner.starts_with('/')
        && inner[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// What to write for a value of the setting: the checked path, or empty (no place). `None`
/// for a value of the wrong type (it changes nothing).
pub fn of(value: &SettingValue) -> Option<String> {
    match value {
        SettingValue::Text(t) => Some(checked(t).unwrap_or_default().to_owned()),
        _ => None,
    }
}

/// Writes `place` to `file` whole: first beside it, then renamed over it, so the model
/// service never reads half a path.
pub fn publish(file: &Path, place: &str) -> std::io::Result<()> {
    let fresh = file.with_extension("new");
    let mut out = std::fs::File::create(&fresh)?;
    out.write_all(place.as_bytes())?;
    out.sync_all()?;
    std::fs::rename(&fresh, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_paths_below_the_model_folders_and_media_pass() {
        assert_eq!(
            checked("/data/ultar/models/mine"),
            Some("/data/ultar/models/mine")
        );
        assert_eq!(
            checked("/run/ultar/media/stick/llm"),
            Some("/run/ultar/media/stick/llm")
        );
        assert_eq!(checked("/data/ultar/agent"), Some("/data/ultar/agent"));
        for bad in [
            "",
            "/etc",
            "/data/ultar/models/../../etc",
            "/data/ultar/models/./x",
            "data/ultar/models",
            "/data/ultar/modelsx",
            "/data/ultar/models/a\nb",
            "/data/ultar/models//x",
            "/data/ultar/models/x/..",
        ] {
            assert_eq!(checked(bad), None, "{bad:?}");
        }
        assert_eq!(
            checked(&format!("/data/ultar/models/{}", "a".repeat(300))),
            None
        );
    }

    #[test]
    fn a_bad_value_empties_the_place_and_a_wrong_type_changes_nothing() {
        let text = |t: &str| of(&SettingValue::Text(t.into()));
        assert_eq!(
            text("/data/ultar/models/x").as_deref(),
            Some("/data/ultar/models/x")
        );
        assert_eq!(text("/etc/shadow").as_deref(), Some(""));
        assert_eq!(text("").as_deref(), Some(""));
        assert_eq!(of(&SettingValue::Flag(true)), None);
    }

    #[test]
    fn the_file_is_replaced_whole() {
        let dir = std::env::temp_dir().join(format!("llmbus-place-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("model-place");
        publish(&file, "/data/ultar/models/a").unwrap();
        publish(&file, "/run/ultar/media/b").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "/run/ultar/media/b"
        );
        assert!(!dir.join("model-place.new").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
