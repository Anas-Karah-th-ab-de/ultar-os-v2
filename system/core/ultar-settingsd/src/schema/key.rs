//! Setting keys and key templates (`display.<id>.scale`).

/// Marker segment that stands for one instance (a display, an app).
pub const INSTANCE_MARKER: &str = "<id>";
/// Longest accepted key in bytes (bus names have the same limit).
pub const MAX_KEY_LEN: usize = 128;
/// Longest accepted single segment in bytes.
pub const MAX_SEGMENT_LEN: usize = 64;

/// `true` when `segment` is non-empty and made of `a-z`, `0-9`, `_`, `-`.
pub fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= MAX_SEGMENT_LEN
        && segment.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

/// `true` when `key` is a dot separated list of valid segments.
pub fn is_valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_KEY_LEN && key.split('.').all(is_valid_segment)
}

/// Turns arbitrary identification text into a valid key segment: lower
/// case, every run of other characters becomes one `-`. Empty input and
/// input without any usable character give `None`.
pub fn sanitize_segment(text: &str) -> Option<String> {
    let mut out = String::new();
    for ch in text.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() || lower == '_' {
            out.push(lower);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= MAX_SEGMENT_LEN {
            break;
        }
    }
    let trimmed = out.trim_end_matches('-');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// A key pattern with at most one [`INSTANCE_MARKER`] segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyTemplate {
    text: &'static str,
    marker: Option<usize>,
}

impl KeyTemplate {
    /// Parses a template; `None` when it is not well formed.
    pub fn parse(text: &'static str) -> Option<KeyTemplate> {
        if text.is_empty() || text.len() > MAX_KEY_LEN {
            return None;
        }
        let mut marker = None;
        for (index, segment) in text.split('.').enumerate() {
            if segment == INSTANCE_MARKER {
                if marker.is_some() || index == 0 {
                    return None;
                }
                marker = Some(index);
            } else if !is_valid_segment(segment) {
                return None;
            }
        }
        Some(KeyTemplate { text, marker })
    }

    /// The template text as declared.
    pub const fn as_str(&self) -> &'static str {
        self.text
    }

    /// Whether the template has an instance segment.
    pub const fn has_instance(&self) -> bool {
        self.marker.is_some()
    }

    /// Matches a concrete key. `Some(None)`: matched a template without
    /// instance; `Some(Some(id))`: matched, with the instance segment.
    pub fn matches<'k>(&self, key: &'k str) -> Option<Option<&'k str>> {
        let Some(marker) = self.marker else {
            return (key == self.text).then_some(None);
        };
        let mut instance = None;
        let mut own = self.text.split('.');
        let mut theirs = key.split('.');
        let mut index = 0;
        loop {
            match (own.next(), theirs.next()) {
                (None, None) => return instance.map(Some),
                (Some(_), Some(segment)) if index == marker => {
                    if !is_valid_segment(segment) {
                        return None;
                    }
                    instance = Some(segment);
                }
                (Some(expected), Some(segment)) if expected == segment => {}
                _ => return None,
            }
            index += 1;
        }
    }

    /// The concrete key for an instance (the template itself without one).
    pub fn instantiate(&self, instance: Option<&str>) -> String {
        match instance {
            Some(id) => self.text.replacen(INSTANCE_MARKER, id, 1),
            None => self.text.to_owned(),
        }
    }

    /// The template without its instance segment: the stem of i18n keys.
    pub fn stem(&self) -> String {
        self.text
            .split('.')
            .filter(|segment| *segment != INSTANCE_MARKER)
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_syntax() {
        assert!(is_valid_key("display.edp-1.scale"));
        assert!(is_valid_key("a"));
        for bad in ["", ".a", "a.", "a..b", "A.b", "a b", "a.<id>.b", "ä"] {
            assert!(!is_valid_key(bad), "{bad}");
        }
        assert!(!is_valid_key(&"a.".repeat(70)));
        assert!(!is_valid_segment(&"a".repeat(65)));
    }

    #[test]
    fn sanitizing_identification_text() {
        assert_eq!(
            sanitize_segment("DEL U2720Q #7").as_deref(),
            Some("del-u2720q-7")
        );
        assert_eq!(sanitize_segment("  --x__y  ").as_deref(), Some("x__y"));
        assert_eq!(sanitize_segment("شاشة"), None);
        assert_eq!(sanitize_segment(""), None);
        let long = sanitize_segment(&"ab ".repeat(100)).unwrap();
        assert!(is_valid_segment(&long));
    }

    #[test]
    fn template_parsing() {
        assert!(KeyTemplate::parse("display.mode").is_some());
        assert!(KeyTemplate::parse("display.<id>.scale").is_some());
        for bad in ["", "<id>.a", "a.<id>.<id>", "a.<x>.b", "a..b", "A"] {
            assert!(KeyTemplate::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn template_matching() {
        let plain = KeyTemplate::parse("display.mode").unwrap();
        assert_eq!(plain.matches("display.mode"), Some(None));
        assert_eq!(plain.matches("display.modes"), None);
        assert!(!plain.has_instance());

        let scale = KeyTemplate::parse("display.<id>.scale").unwrap();
        assert_eq!(scale.matches("display.edp-1.scale"), Some(Some("edp-1")));
        assert_eq!(scale.matches("display.scale"), None);
        assert_eq!(scale.matches("display.a.b.scale"), None);
        assert_eq!(scale.matches("display.a.scale.x"), None);
        assert_eq!(scale.matches("display.A.scale"), None);
        assert_eq!(scale.matches("display.<id>.scale"), None);
        assert!(scale.has_instance());
    }

    #[test]
    fn instantiate_and_stem() {
        let scale = KeyTemplate::parse("display.<id>.scale").unwrap();
        assert_eq!(scale.instantiate(Some("hdmi")), "display.hdmi.scale");
        assert_eq!(scale.instantiate(None), "display.<id>.scale");
        assert_eq!(scale.stem(), "display.scale");
        assert_eq!(scale.as_str(), "display.<id>.scale");
        let last = KeyTemplate::parse("a.<id>").unwrap();
        assert_eq!(last.matches("a.x"), Some(Some("x")));
        assert_eq!(last.stem(), "a");
    }
}
