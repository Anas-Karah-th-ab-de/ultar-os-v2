//! The DNS configuration of the device: `resolv.conf` written by this
//! service (atomically, mode 0644) from the VPN's servers first, then those
//! of the DHCP lease; at most three, as the resolver reads no more.

use std::io::Write;
use std::net::IpAddr;
use std::path::Path;

/// Most servers written.
pub const MAX_SERVERS: usize = 3;

/// The servers to write: `first` (the VPN), then `then` (DHCP), without
/// repeats, only valid addresses, at most [`MAX_SERVERS`].
pub fn servers(first: &[String], then: &[String]) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    for s in first.iter().chain(then) {
        if let Ok(a) = s.parse::<IpAddr>() {
            if !out.contains(&a) && out.len() < MAX_SERVERS {
                out.push(a);
            }
        }
    }
    out
}

/// The file's text.
pub fn render(servers: &[IpAddr]) -> String {
    let mut text = String::from("# written by ultar-netd\n");
    for s in servers {
        text.push_str("nameserver ");
        text.push_str(&s.to_string());
        text.push('\n');
    }
    text
}

/// Writes `path` atomically (temporary file, fsync, rename, directory fsync).
pub fn write(path: &Path, servers: &[IpAddr]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    let tmp = dir.join(".resolv.conf.tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&tmp)?;
    f.write_all(render(servers).as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpn_first_no_repeats_at_most_three() {
        let list = servers(
            &["10.8.0.1".into(), "bad".into()],
            &[
                "192.168.1.1".into(),
                "10.8.0.1".into(),
                "1.1.1.1".into(),
                "9.9.9.9".into(),
            ],
        );
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].to_string(), "10.8.0.1");
        let text = render(&list);
        assert!(text.contains("nameserver 192.168.1.1\n"));
        let dir = std::env::temp_dir().join(format!("netd-dns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("resolv.conf");
        write(&path, &list).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(write(Path::new("/"), &list).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
