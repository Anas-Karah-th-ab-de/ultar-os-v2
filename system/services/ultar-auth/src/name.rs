//! Account names and display names.

use ultar_bus::messages::auth::{MAX_DISPLAY_NAME, MAX_USER_NAME};

/// A valid account name: 1–32 bytes, starts with a lower-case letter, then lower-case
/// letters, digits, `-`, `_`, `.`.
pub fn valid_user(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    first_ok
        && name.len() <= MAX_USER_NAME
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// A valid display name: 1–128 bytes without control characters.
pub fn valid_display(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= MAX_DISPLAY_NAME && !name.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert!(valid_user("anas") && valid_user("a.b-c_9"));
        for bad in ["", "Anas", "9a", "a b", "a/b", "ä", &"a".repeat(33)] {
            assert!(!valid_user(bad), "{bad}");
        }
        assert!(valid_display("أنس قرة محمد") && valid_display("Leila"));
        assert!(!valid_display("") && !valid_display("  ") && !valid_display("a\nb"));
        assert!(!valid_display(&"x".repeat(129)));
    }
}
