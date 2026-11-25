//! Version numbers of upstream releases: which tag is a release, and which release is newer.

/// The numeric parts of a version (`1.3.1` → `[1, 3, 1]`); `None` if it is not made of numbers
/// and dots only – release candidates, betas and dated snapshots are not releases.
pub fn parse(version: &str) -> Option<Vec<u64>> {
    if version.is_empty() {
        return None;
    }
    version.split('.').map(|p| p.parse().ok()).collect()
}

/// The version a tag names under `pattern`, where `{version}` stands for the number
/// (`v{version}`, `libdrm-{version}`) and `{version_}` for the number written with
/// underscores (`rel_{version_}` for `rel_1_3_5`). `None`: not such a tag.
pub fn of_tag(tag: &str, pattern: &str) -> Option<String> {
    let (marker, underscores) = match pattern.contains("{version_}") {
        true => ("{version_}", true),
        false => ("{version}", false),
    };
    let (before, after) = pattern.split_once(marker)?;
    let inner = tag.strip_prefix(before)?.strip_suffix(after)?;
    let version = match underscores {
        true if !inner.contains('.') => inner.replace('_', "."),
        true => return None,
        false => inner.to_string(),
    };
    parse(&version).map(|_| version)
}

/// `text` with `{version}` and `{version_}` filled in.
pub fn fill(text: &str, version: &str) -> String {
    text.replace("{version_}", &version.replace('.', "_"))
        .replace("{version}", version)
}

/// The newest release among `tags`.
pub fn newest(tags: &[String], pattern: &str) -> Option<String> {
    let versions = tags.iter().filter_map(|t| of_tag(t, pattern));
    versions.max_by_key(|v| parse(v).unwrap_or_default())
}

/// Whether `candidate` is newer than `pinned`.
pub fn newer(candidate: &str, pinned: &str) -> bool {
    match (parse(candidate), parse(pinned)) {
        (Some(c), Some(p)) => c > p,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_are_numbers_and_dots() {
        assert_eq!(parse("1.3.1"), Some(vec![1, 3, 1]));
        assert_eq!(parse("25.2"), Some(vec![25, 2]));
        assert_eq!(parse("1.3.1-rc2"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn tags_are_matched_by_their_pattern() {
        assert_eq!(of_tag("v1.3.1", "v{version}"), Some("1.3.1".into()));
        assert_eq!(
            of_tag("mesa-25.2.4", "mesa-{version}"),
            Some("25.2.4".into())
        );
        assert_eq!(of_tag("mesa-25.3.0-rc1", "mesa-{version}"), None);
        assert_eq!(of_tag("R_2_7_1", "v{version}"), None);
        assert_eq!(of_tag("v1.4.321.0", "v{version}"), Some("1.4.321.0".into()));
        assert_eq!(of_tag("rel_1_3_5", "rel_{version_}"), Some("1.3.5".into()));
        assert_eq!(of_tag("rel_1.3.5", "rel_{version_}"), None);
        assert_eq!(
            fill("a/rel_{version_}/m-{version}.tgz", "1.3.5"),
            "a/rel_1_3_5/m-1.3.5.tgz"
        );
    }

    #[test]
    fn newest_compares_numbers_not_text() {
        let tags: Vec<String> = ["v1.9.0", "v1.10.0", "v1.10.0-rc1", "junk"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(newest(&tags, "v{version}"), Some("1.10.0".into()));
        assert!(newer("1.10.0", "1.9.0"));
        assert!(!newer("1.9.0", "1.9.0"));
        assert!(!newer("2.0-beta", "1.9.0"));
    }
}
