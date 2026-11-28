//! The files and folders of the person's desktop folder (the file area `desktop`,
//! `/data/ultar/areas/desktop`), shown on the background after the shortcuts (CLICKS
//! `desk.icon`). The shell only reads the folder: opening, renaming, moving to the trash and
//! the properties are Files' (`files://desktop/<name>?action=…`), a file opens with its
//! default app (`launcher.resolve`). Hidden names (a leading dot) and links are not shown.

use std::path::Path;
use ultar_ui::icon::IconId;

/// Most items shown.
pub const MAX_FILES: usize = 64;
/// The file area of the desktop folder (`files://desktop/…`).
pub const AREA: &str = "desktop";

/// One item of the desktop folder.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeskFile {
    /// Its name in the folder.
    pub name: String,
    /// A folder (else a file).
    pub folder: bool,
}

/// The items of `dir`: folders first, then files, each in the alphabetical order of their
/// names; at most [`MAX_FILES`]. A missing or unreadable folder has none.
pub fn list(dir: &Path) -> Vec<DeskFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<DeskFile> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let kind = e.file_type().ok()?;
            let shown = !name.starts_with('.') && !kind.is_symlink() && !name.contains('/');
            shown.then_some(DeskFile {
                name,
                folder: kind.is_dir(),
            })
        })
        .collect();
    items.sort_by_cached_key(|f| {
        (
            !f.folder,
            ultar_i18n::collation::collation_key(&f.name),
            f.name.clone(),
        )
    });
    items.truncate(MAX_FILES);
    items
}

/// The `files://` URI of an item (Files' form: the name percent-encoded).
pub fn uri(name: &str) -> String {
    format!("files://{AREA}/{}", encode(name))
}

/// The URI of an item with an action of Files (`rename`, `trash`, `properties`).
pub fn action_uri(name: &str, action: &str) -> String {
    format!("{}?action={action}", uri(name))
}

/// Percent-encodes a name like Files does (letters and digits of any script, `-._~` stay).
fn encode(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for (i, ch) in part.char_indices() {
        let b = part.as_bytes()[i];
        if !ch.is_ascii() || b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(ch);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The icon of an item by its kind (Files' kinds: folder, text, sheet, picture, sound,
/// film, archive, web page).
pub fn icon(f: &DeskFile) -> IconId {
    if f.folder {
        return IconId::KindFolder;
    }
    let lower = f.name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => IconId::KindImage,
        "xlsx" | "xls" | "ods" | "csv" => IconId::KindSheet,
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => IconId::Media,
        "mp4" | "mkv" | "webm" | "avi" | "mov" => IconId::Video,
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "uap" | "deb" => IconId::Package,
        "html" | "htm" => IconId::Browser,
        _ => IconId::KindText,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_lists_folders_first_without_hidden_items() {
        let dir = std::env::temp_dir().join(format!("desk-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Projekte")).unwrap();
        for name in ["zeit.pdf", "Bild.png", ".geheim", "عقد.docx"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let names: Vec<(String, bool)> =
            list(&dir).into_iter().map(|f| (f.name, f.folder)).collect();
        assert_eq!(names[0], ("Projekte".to_owned(), true), "folders first");
        assert_eq!(names.len(), 4, "the hidden file is left out: {names:?}");
        assert!(!names.iter().any(|(n, _)| n.starts_with('.')));
        assert!(list(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uris_and_icons_follow_files() {
        assert_eq!(uri("عقد 2026.pdf"), "files://desktop/عقد%202026.pdf");
        assert_eq!(uri("a?b#c"), "files://desktop/a%3Fb%23c");
        assert_eq!(
            action_uri("x.txt", "rename"),
            "files://desktop/x.txt?action=rename"
        );
        let file = |n: &str| DeskFile {
            name: n.into(),
            folder: false,
        };
        assert_eq!(icon(&file("a.PNG")), IconId::KindImage);
        assert_eq!(icon(&file("a.ods")), IconId::KindSheet);
        assert_eq!(icon(&file("notes")), IconId::KindText);
        let folder = DeskFile {
            name: "x".into(),
            folder: true,
        };
        assert_eq!(icon(&folder), IconId::KindFolder);
    }
}
