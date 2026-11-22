//! Rule `logical-layout`: layout code uses start/end, not left/right (plan §3.4b, §7 rule 0c).

use super::Rule;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::scan::{SourceFile, TokenKind};

/// Areas that contain layout code.
const CHECKED_PREFIXES: [&str; 2] = ["platform/", "apps/"];

/// Identifier parts that name a physical side.
const PHYSICAL_SIDES: [&str; 2] = ["left", "right"];

/// Comment marker that allows a physical side on that line.
const MARKER: &str = "physical";

/// Flags identifiers with a `left`/`right` part unless the line is marked `// physical`.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files.iter().filter(|f| f.is_under(&CHECKED_PREFIXES)) {
        let mut last_line = 0;
        for token in &file.code {
            let candidate = token.kind == TokenKind::Ident
                && !token.in_test
                && token.line != last_line
                && names_physical_side(&token.text);
            if candidate && !is_marked(file, token.line) {
                let message = Message::new(Key::PhysicalSide, &[&token.text]);
                let finding = Finding::error(Rule::LogicalLayout, &file.path, token.line, message);
                findings.push(finding);
                last_line = token.line;
            }
        }
    }
    findings
}

fn is_marked(file: &SourceFile, line: u32) -> bool {
    file.comments
        .iter()
        .any(|c| c.line <= line && c.end_line >= line && c.text.contains(MARKER))
}

/// Whether one snake_case / camelCase part of the identifier is `left` or `right`.
pub fn names_physical_side(ident: &str) -> bool {
    identifier_parts(ident)
        .iter()
        .any(|part| PHYSICAL_SIDES.contains(&part.as_str()))
}

/// Splits an identifier into lower-cased words at `_` and camelCase boundaries.
pub fn identifier_parts(ident: &str) -> Vec<String> {
    let chars: Vec<char> = ident.chars().collect();
    let mut parts = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            flush(&mut current, &mut parts);
            continue;
        }
        let previous = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
        let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        let boundary = c.is_uppercase()
            && previous.is_some_and(|p| {
                p.is_lowercase() || p.is_ascii_digit() || (p.is_uppercase() && next_is_lower)
            });
        if boundary {
            flush(&mut current, &mut parts);
        }
        current.extend(c.to_lowercase());
    }
    flush(&mut current, &mut parts);
    parts
}

fn flush(current: &mut String, parts: &mut Vec<String>) {
    if !current.is_empty() {
        parts.push(std::mem::take(current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_split_into_words() {
        assert_eq!(identifier_parts("padding_left"), ["padding", "left"]);
        assert_eq!(identifier_parts("paddingLeft"), ["padding", "left"]);
        assert_eq!(identifier_parts("LeftPane"), ["left", "pane"]);
        assert_eq!(identifier_parts("LEFT_MARGIN"), ["left", "margin"]);
        assert_eq!(identifier_parts("HTMLRightEdge"), ["html", "right", "edge"]);
        assert_eq!(identifier_parts("x2Left"), ["x2", "left"]);
        assert_eq!(identifier_parts("__"), Vec::<String>::new());
    }

    #[test]
    fn only_whole_words_count() {
        assert!(names_physical_side("left"));
        assert!(names_physical_side("margin_right"));
        assert!(names_physical_side("RightToLeft"));
        assert!(!names_physical_side("leftover"));
        assert!(!names_physical_side("copyright"));
        assert!(!names_physical_side("brightness"));
        assert!(!names_physical_side("uprights"));
        assert!(!names_physical_side("start"));
    }
}
