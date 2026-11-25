//! Which addresses a page may reach: `http` and `https` to the internet, never this device or
//! the local network (a page could otherwise talk to the system's own services, the router or
//! other devices: "nothing from the internet reaches the system"). Checked for every address
//! and every redirect.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why an address is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// Not `http` or `https` (`file:`, `ftp:`, `javascript:` …).
    Scheme,
    /// This device or the local network.
    Local,
    /// Not a valid address.
    Invalid,
}

/// Checks `url` before it is fetched.
pub fn check(url: &str) -> Result<(), Blocked> {
    let lower = url.trim().to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .ok_or(Blocked::Scheme)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // user:pass@ in an address hides where it goes
    if authority.contains('@') || authority.is_empty() {
        return Err(Blocked::Invalid);
    }
    let host = host_of(authority).ok_or(Blocked::Invalid)?;
    if local_name(host) {
        return Err(Blocked::Local);
    }
    match parse_ip(host) {
        Some(ip) if local_ip(ip) => Err(Blocked::Local),
        _ => Ok(()),
    }
}

/// The host of `host[:port]` or `[v6][:port]`.
fn host_of(authority: &str) -> Option<&str> {
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split(']').next().filter(|h| !h.is_empty());
    }
    let host = authority.rsplit_once(':').map_or(authority, |(h, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            authority
        }
    });
    (!host.is_empty()).then_some(host.trim_end_matches('.'))
}

fn local_name(host: &str) -> bool {
    host == "localhost"
        || [
            ".localhost",
            ".local",
            ".lan",
            ".home",
            ".internal",
            ".arpa",
        ]
        .iter()
        .any(|s| host.ends_with(s))
        || !host.contains('.') && parse_ip(host).is_none()
}

/// IPv4 also in the forms browsers accept (`2130706433`, `0x7f.1`, `127.1`).
fn parse_ip(host: &str) -> Option<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(ip);
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let nums: Option<Vec<u64>> = parts.iter().map(|p| number(p)).collect();
    let nums = nums?;
    let (last, front) = nums.split_last()?;
    if front.iter().any(|n| *n > 255) {
        return None;
    }
    let mut value: u64 = 0;
    for n in front {
        value = (value << 8) | n;
    }
    let shift = 8 * (4 - nums.len() as u64 + 1);
    let full = (value << shift) | last;
    u32::try_from(full)
        .ok()
        .map(|v| IpAddr::V4(Ipv4Addr::from(v)))
}

fn number(p: &str) -> Option<u64> {
    if let Some(hex) = p.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else if p.len() > 1 && p.starts_with('0') {
        u64::from_str_radix(&p[1..], 8).ok()
    } else {
        p.parse().ok()
    }
}

fn local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => local_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return local_v4(v4);
            }
            let seg = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg & 0xfe00) == 0xfc00 // unique local
                || (seg & 0xffc0) == 0xfe80 // link local
                || v6 == Ipv6Addr::LOCALHOST
        }
    }
}

fn local_v4(v4: Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b)) // shared address space (carrier NAT)
        || (a == 198 && (18..20).contains(&b)) // benchmarking
        || a >= 240
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_internet_is_allowed() {
        for ok in [
            "https://ar.wikipedia.org/wiki/سوريا",
            "http://example.com:8080/x",
            "https://8.8.8.8/",
            "https://[2606:4700::1111]/",
        ] {
            assert_eq!(check(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn this_device_and_the_local_network_are_not() {
        for local in [
            "http://localhost/",
            "http://127.0.0.1:8080/",
            "http://2130706433/",
            "http://0x7f.1/",
            "http://127.1/",
            "http://0177.0.0.1/",
            "http://192.168.100.18/",
            "http://10.0.0.1/",
            "http://172.16.5.5/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fe80::1]/",
            "http://router.local/",
            "http://printer/",
            "http://0.0.0.0/",
        ] {
            assert_eq!(check(local), Err(Blocked::Local), "{local}");
        }
        assert_eq!(check("file:///etc/passwd"), Err(Blocked::Scheme));
        assert_eq!(check("javascript:alert(1)"), Err(Blocked::Scheme));
        assert_eq!(
            check("https://user@evil.com@127.0.0.1/"),
            Err(Blocked::Invalid)
        );
    }
}
