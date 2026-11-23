//! Start and stop order from the `after` lists.
//!
//! A service's **depth** is the length of the longest chain of services it
//! is ordered after (0: none). Services start when their dependencies are
//! up; at shutdown the deepest stop first and the log service (depth 0,
//! everything logs to it) last. A dependency cycle cannot start in any
//! order: its members are reported and never started.

use std::collections::BTreeMap;

/// Depths of the services, and the members of dependency cycles.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Order {
    /// Depth of every service that is not in a cycle.
    pub depth: BTreeMap<String, usize>,
    /// Services in (or behind) a dependency cycle.
    pub cyclic: Vec<String>,
}

/// Computes the order of `services` (name → names it is ordered after).
/// Unknown names in an `after` list do not hold a service back.
pub fn order(services: &BTreeMap<String, Vec<String>>) -> Order {
    let mut result = Order::default();
    let mut remaining: BTreeMap<&str, Vec<&str>> = services
        .iter()
        .map(|(name, after)| {
            let known = after
                .iter()
                .map(String::as_str)
                .filter(|dep| services.contains_key(*dep))
                .collect();
            (name.as_str(), known)
        })
        .collect();
    // Repeatedly settle the services whose dependencies are all settled.
    loop {
        let ready: Vec<(&str, usize)> = remaining
            .iter()
            .filter_map(|(name, after)| {
                let depths: Option<Vec<usize>> = after
                    .iter()
                    .map(|dep| result.depth.get(*dep).copied())
                    .collect();
                depths.map(|depths| (*name, depths.into_iter().map(|d| d + 1).max().unwrap_or(0)))
            })
            .collect();
        if ready.is_empty() {
            break;
        }
        for (name, depth) in ready {
            remaining.remove(name);
            result.depth.insert(name.to_owned(), depth);
        }
    }
    result.cyclic = remaining.keys().map(|name| (*name).to_owned()).collect();
    result
}

impl Order {
    /// Names grouped by depth, deepest first: the stop order.
    pub fn stop_waves(&self) -> Vec<Vec<String>> {
        let deepest = self.depth.values().copied().max().unwrap_or(0);
        (0..=deepest)
            .rev()
            .map(|level| {
                self.depth
                    .iter()
                    .filter(|(_, depth)| **depth == level)
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>()
            })
            .filter(|wave| !wave.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(name, after)| {
                let after = after.iter().map(|dep| (*dep).to_owned()).collect();
                ((*name).to_owned(), after)
            })
            .collect()
    }

    #[test]
    fn depths_follow_the_longest_chain_and_stop_in_reverse() {
        let order = order(&graph(&[
            ("logd", &[]),
            ("bus", &["logd"]),
            ("settingsd", &["bus"]),
            ("devd", &["bus", "logd"]),
            ("ui", &["coldplug"]),
        ]));
        assert_eq!(order.depth["logd"], 0);
        assert_eq!(order.depth["bus"], 1);
        assert_eq!(order.depth["devd"], 2);
        // An unknown dependency does not hold a service back.
        assert_eq!(order.depth["ui"], 0);
        assert!(order.cyclic.is_empty());
        assert_eq!(
            order.stop_waves(),
            vec![
                vec!["devd".to_owned(), "settingsd".to_owned()],
                vec!["bus".to_owned()],
                vec!["logd".to_owned(), "ui".to_owned()],
            ]
        );
    }

    #[test]
    fn cycles_and_what_depends_on_them_are_reported() {
        let order = order(&graph(&[
            ("a", &["b"]),
            ("b", &["a"]),
            ("c", &["a"]),
            ("d", &[]),
            ("e", &["e"]),
        ]));
        assert_eq!(order.cyclic, ["a", "b", "c", "e"]);
        assert_eq!(order.depth.keys().collect::<Vec<_>>(), ["d"]);
        assert!(super::order(&BTreeMap::new()).stop_waves().is_empty());
    }
}
