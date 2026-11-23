//! The example reverse-proxy configurations in `deploy/` carry the limits that DEPLOY.md relies
//! on (review round 4, R4-4). This checks the files; it does not run Caddy.

use std::path::Path;

fn read(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("deploy")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// The number after `directive` on its line (`max_conns_per_host 2048` → 2048).
fn numbers(text: &str, directive: &str) -> Vec<u64> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix(directive))
        .filter_map(|rest| {
            rest.trim()
                .trim_end_matches(|c: char| c.is_ascii_alphabetic())
                .parse()
                .ok()
        })
        .collect()
}

#[test]
fn both_caddyfiles_cut_slow_clients_and_stay_below_the_connection_cap() {
    for name in ["Caddyfile", "Caddyfile.docker"] {
        let text = read(name);
        assert_eq!(numbers(&text, "read_header"), [10], "{name}");
        assert_eq!(numbers(&text, "read_body"), [30], "{name}");
        assert!(text.contains("max_header_size 16KB"), "{name}");
        assert!(text.contains("max_size 64KB"), "{name}");
        let per_host = numbers(&text, "max_conns_per_host");
        assert_eq!(per_host.len(), 2, "{name}: both reverse_proxy blocks");
        let total: u64 = per_host.iter().sum();
        assert!(
            total < ultraos_server::server::DEFAULT_MAX_CONNECTIONS as u64,
            "{name}: Caddy may open {total} connections"
        );
        // header read time at Caddy is not longer than the server's own
        assert!(
            10 <= ultraos_server::server::HEADER_READ_TIMEOUT.as_secs(),
            "{name}"
        );
        // braces balance (a cheap guard against a broken edit)
        assert_eq!(
            text.matches('{').count(),
            text.matches('}').count(),
            "{name}"
        );
    }
}

#[test]
fn the_proxy_is_trusted_by_default_so_its_connections_are_not_capped_per_client() {
    let config = ultraos_server::Config::new("x", "y", [0; 32]);
    let loopback = "127.0.0.1".parse().unwrap();
    assert!(config
        .trusted_proxies
        .iter()
        .any(|net| net.contains(loopback)));
}
