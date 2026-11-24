//! Names typed by people or read from disk: checked before they are used as one path
//! component, and free names for copies ("name 2.txt").

use super::FsError;

/// Longest name in bytes (the limit of common file systems).
pub const MAX_NAME: usize = 255;

/// Checks a single name: not empty, not `.` or `..`, no `/`, no NUL, at most [`MAX_NAME`]
/// bytes, no control characters (they break lines and terminals).
pub fn check_name(name: &str) -> Result<(), FsError> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > MAX_NAME
        || name.chars().any(|c| c == '/' || c.is_control());
    if bad {
        Err(FsError::BadName)
    } else {
        Ok(())
    }
}

/// The first of `name`, `stem 2.ext`, `stem 3.ext`, … for which `taken` is false
/// (`None` after 9999 tries or when no such name fits [`MAX_NAME`]).
pub fn free_name(name: &str, mut taken: impl FnMut(&str) -> bool) -> Option<String> {
    if !taken(name) {
        return Some(name.to_owned());
    }
    // A leading dot is not an extension (".profile").
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    (2..10_000u32)
        .map(|n| format!("{stem} {n}{ext}"))
        .filter(|c| c.len() <= MAX_NAME)
        .find(|c| !taken(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        for ok in ["a", "تقرير.docx", "a b", ".hidden", "مزيج mixed ١٢٣"] {
            assert_eq!(check_name(ok), Ok(()), "{ok}");
        }
        let long = "a".repeat(MAX_NAME + 1);
        for bad in ["", ".", "..", "a/b", "a\0", "a\nb", long.as_str()] {
            assert_eq!(check_name(bad), Err(FsError::BadName), "{bad:?}");
        }
    }

    #[test]
    fn free_names_count_up_before_the_extension() {
        let taken = ["r.txt", "r 2.txt"];
        assert_eq!(
            free_name("r.txt", |n| taken.contains(&n)).unwrap(),
            "r 3.txt"
        );
        assert_eq!(free_name("dir", |n| n == "dir").unwrap(), "dir 2");
        assert_eq!(free_name(".rc", |n| n == ".rc").unwrap(), ".rc 2");
        assert_eq!(free_name("x", |_| false).unwrap(), "x");
        assert_eq!(free_name("x", |_| true), None);
    }
}
