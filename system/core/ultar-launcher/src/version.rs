//! Versions of apps: the order an update must keep (review L-1).

/// The numbers of a version core (`None`: a part is not a number).
fn numbers(core: &str) -> Option<Vec<u64>> {
    core.split('.').map(|part| part.parse().ok()).collect()
}

/// The order of two versions (`1.4.0`, `2.0.0-beta`, `1.0+build`): parts
/// as numbers (missing ones 0), `+build` ignored, a pre-release before its
/// release. `None`: a part is not a number (only equal texts are equal).
pub fn order(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let split = |v: &str| {
        let v = v.split('+').next().unwrap_or(v);
        match v.split_once('-') {
            Some((core, pre)) => (core.to_owned(), Some(pre.to_owned())),
            None => (v.to_owned(), None),
        }
    };
    let ((core_a, pre_a), (core_b, pre_b)) = (split(a), split(b));
    let (mut na, mut nb) = (numbers(&core_a)?, numbers(&core_b)?);
    let len = na.len().max(nb.len());
    na.resize(len, 0);
    nb.resize(len, 0);
    Some(na.cmp(&nb).then(match (pre_a, pre_b) {
        (None, None) => std::cmp::Ordering::Equal,
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => x.cmp(&y),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_ordered_like_semver() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        assert_eq!(order("1.4.0", "1.10.0"), Some(Less));
        assert_eq!(order("2", "1.9.9"), Some(Greater));
        assert_eq!(order("1.0", "1.0.0"), Some(Equal));
        assert_eq!(order("1.0.0-beta", "1.0.0"), Some(Less));
        assert_eq!(order("1.0.0-beta", "1.0.0-alpha"), Some(Greater));
        assert_eq!(order("1.0.0+7", "1.0.0+8"), Some(Equal));
        assert_eq!(order("1.x", "1.0"), None);
    }
}
