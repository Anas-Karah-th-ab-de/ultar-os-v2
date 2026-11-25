//! Where documents live: the cell binds each granted place of the user at `/areas/<name>`
//! (`home`, `documents`, `removable/<drive>` …); a `files://<area>/<path>` URI names a file
//! below one of them. Nothing outside the areas is reachable (no `..`, no absolute parts).

use std::path::{Path, PathBuf};

/// Longest URI taken.
pub const MAX_URI: usize = 4096;
/// The area new documents are saved in.
pub const NEW_IN: &str = "documents";

/// The path of the file a `files://` URI names below `areas`; `None` for anything else.
pub fn path_of(uri: &str, areas: &Path) -> Option<PathBuf> {
    if uri.len() > MAX_URI || uri.chars().any(char::is_control) {
        return None;
    }
    let rest = uri.strip_prefix("files://")?;
    let mut path = areas.to_path_buf();
    let mut parts = 0;
    for raw in rest.split('/').filter(|p| !p.is_empty()) {
        let part = decode(raw)?;
        if part == "." || part == ".." || part.contains('/') || part.contains('\0') {
            return None;
        }
        path.push(part);
        parts += 1;
    }
    // an area and at least one name in it
    (parts >= 2).then_some(path)
}

/// Where a new document called `name` is saved (in [`NEW_IN`]); `None` for a bad name.
pub fn new_path(name: &str, areas: &Path) -> Option<PathBuf> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.chars().any(char::is_control);
    if bad {
        return None;
    }
    let name = if name.contains('.') {
        name.to_owned()
    } else {
        format!("{name}.txt")
    };
    Some(areas.join(NEW_IN).join(name))
}

fn decode(part: &str) -> Option<String> {
    let raw = part.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = std::str::from_utf8(raw.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_uri_is_a_path_below_the_areas() {
        let a = Path::new("/areas");
        assert_eq!(
            path_of(
                "files://documents/%D9%85%D9%84%D8%A7%D8%AD%D8%B8%D8%A7%D8%AA.txt",
                a
            ),
            Some(PathBuf::from("/areas/documents/ملاحظات.txt"))
        );
        assert_eq!(
            path_of("files://removable/USB/a%20b.md", a),
            Some(PathBuf::from("/areas/removable/USB/a b.md"))
        );
        assert_eq!(path_of("files://documents/../x", a), None);
        assert_eq!(path_of("files://documents/%2E%2E/x", a), None);
        assert_eq!(path_of("files://documents/a%2Fb", a), None);
        assert_eq!(path_of("files://documents", a), None);
        assert_eq!(path_of("file:///etc/passwd", a), None);
    }

    #[test]
    fn new_documents_go_to_documents_with_txt() {
        let a = Path::new("/areas");
        assert_eq!(
            new_path("ملاحظات", a),
            Some(PathBuf::from("/areas/documents/ملاحظات.txt"))
        );
        assert_eq!(
            new_path("a.md", a),
            Some(PathBuf::from("/areas/documents/a.md"))
        );
        assert_eq!(new_path("../x", a), None);
        assert_eq!(new_path("  ", a), None);
    }
}
