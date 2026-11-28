//! Where workbooks live: the cell binds each granted place of the user at `/areas/<name>`
//! (`home`, `documents`, `removable/<drive>` …); a `files://<area>/<path>` URI names a file
//! below one of them, optionally with a position `#sheet=<name>&cell=<A1>` (what the device
//! mesh hands over: the workbook, the sheet and the cell). Nothing outside the areas is
//! reachable (no `..`, no absolute parts).

use std::path::{Path, PathBuf};

/// Longest URI taken.
pub const MAX_URI: usize = 4096;
/// The area new workbooks are saved in.
pub const NEW_IN: &str = "documents";

/// A place inside a workbook.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spot {
    /// Sheet name, when given.
    pub sheet: Option<String>,
    /// Cell (row, column), when given.
    pub cell: Option<(u32, u32)>,
}

/// The URI without its fragment, and the place the fragment names.
pub fn split(uri: &str) -> (&str, Spot) {
    let Some((base, frag)) = uri.split_once('#') else {
        return (uri, Spot::default());
    };
    let mut spot = Spot::default();
    for part in frag.split('&') {
        match part.split_once('=') {
            Some(("sheet", v)) => spot.sheet = decode(v).filter(|s| !s.is_empty()),
            Some(("cell", v)) => {
                spot.cell = ultar_office::sheet::parse_cell(v).filter(|_| v.len() <= 12);
            }
            _ => {}
        }
    }
    (base, spot)
}

/// The URI of a workbook at a place (the inverse of [`split`]).
pub fn with_spot(base: &str, sheet: &str, row: u32, col: u32) -> String {
    format!(
        "{base}#sheet={}&cell={}",
        encode(sheet),
        ultar_office::sheet::cell_name(row, col)
    )
}

/// The `files://` URI of `path` below `areas`; `None` for a path outside of them.
pub fn uri_of(path: &Path, areas: &Path) -> Option<String> {
    let rel = path.strip_prefix(areas).ok()?;
    let mut uri = String::from("files:/");
    for part in rel.components() {
        uri.push('/');
        uri.push_str(&encode(&part.as_os_str().to_string_lossy()));
    }
    Some(uri)
}

/// The path of the file a `files://` URI names below `areas`; `None` for anything else.
pub fn path_of(uri: &str, areas: &Path) -> Option<PathBuf> {
    let uri = split(uri).0;
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

/// Where a workbook called `name` is saved (in [`NEW_IN`]); `None` for a bad name. A name
/// without a dot gets `.xlsx`.
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
        format!("{name}.xlsx")
    };
    Some(areas.join(NEW_IN).join(name))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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
            path_of("files://documents/%D9%85%D9%8A%D8%B2%D8%A7%D9%86%D9%8A%D8%A9.xlsx", a),
            Some(PathBuf::from("/areas/documents/ميزانية.xlsx"))
        );
        assert_eq!(path_of("files://documents/../x", a), None);
        assert_eq!(path_of("files://documents/%2E%2E/x", a), None);
        assert_eq!(path_of("files://documents/a%2Fb", a), None);
        assert_eq!(path_of("files://documents", a), None);
        assert_eq!(path_of("file:///etc/passwd", a), None);
    }

    #[test]
    fn a_position_travels_in_the_fragment() {
        let (base, spot) = split("files://documents/a.xlsx#sheet=Blatt%201&cell=C7");
        assert_eq!(base, "files://documents/a.xlsx");
        assert_eq!(spot.sheet.as_deref(), Some("Blatt 1"));
        assert_eq!(spot.cell, Some((6, 2)));
        assert_eq!(
            path_of("files://documents/a.xlsx#cell=A1", Path::new("/areas")),
            Some(PathBuf::from("/areas/documents/a.xlsx"))
        );
        assert_eq!(split("files://documents/a.xlsx").1, Spot::default());
        assert_eq!(split("x#cell=ZZZZZ99999999").1.cell, None);
        let uri = with_spot("files://documents/a.xlsx", "ورقة 1", 6, 2);
        assert_eq!(split(&uri).1.sheet.as_deref(), Some("ورقة 1"));
        assert_eq!(split(&uri).1.cell, Some((6, 2)));
    }

    #[test]
    fn a_path_below_the_areas_has_a_uri() {
        let a = Path::new("/areas");
        assert_eq!(
            uri_of(Path::new("/areas/documents/a b.xlsx"), a).as_deref(),
            Some("files://documents/a%20b.xlsx")
        );
        assert_eq!(uri_of(Path::new("/etc/x"), a), None);
    }

    #[test]
    fn new_workbooks_go_to_documents_with_xlsx() {
        let a = Path::new("/areas");
        assert_eq!(new_path("ميزانية", a), Some(PathBuf::from("/areas/documents/ميزانية.xlsx")));
        assert_eq!(new_path("a.csv", a), Some(PathBuf::from("/areas/documents/a.csv")));
        assert_eq!(new_path("../x", a), None);
        assert_eq!(new_path("  ", a), None);
    }
}
