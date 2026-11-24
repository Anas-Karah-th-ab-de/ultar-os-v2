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

/// A DNS question for the A record of `name` (recursion desired) with id `id`.
pub fn query(name: &str, id: u16) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    Some(q)
}

/// The first A record of an answer to question `id` (`None`: no answer, an error, no A).
pub fn first_a(reply: &[u8], id: u16) -> Option<std::net::Ipv4Addr> {
    if reply.len() < 12
        || reply[..2] != id.to_be_bytes()
        || reply[2] & 0x80 == 0
        || reply[3] & 0x0f != 0
    {
        return None;
    }
    let questions = u16::from_be_bytes([reply[4], reply[5]]);
    let answers = u16::from_be_bytes([reply[6], reply[7]]);
    let mut p = 12;
    let skip_name = |mut p: usize| -> Option<usize> {
        loop {
            let l = *reply.get(p)? as usize;
            if l & 0xc0 == 0xc0 {
                return Some(p + 2);
            }
            if l == 0 {
                return Some(p + 1);
            }
            p += l + 1;
        }
    };
    for _ in 0..questions {
        p = skip_name(p)? + 4;
    }
    for _ in 0..answers {
        p = skip_name(p)?;
        let head = reply.get(p..p + 10)?;
        let (kind, len) = (
            u16::from_be_bytes([head[0], head[1]]),
            u16::from_be_bytes([head[8], head[9]]) as usize,
        );
        p += 10;
        let data = reply.get(p..p + len)?;
        if kind == 1 && len == 4 {
            return Some(std::net::Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        p += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_and_its_answer() {
        let q = query("ultar.code-craft.tech", 0x1234).unwrap();
        assert_eq!(&q[..4], &[0x12, 0x34, 1, 0]);
        assert_eq!(q[12], 5);
        assert_eq!(&q[13..18], b"ultar");
        assert!(query("bad..name", 1).is_none());
        // the answer: header, the question again, a CNAME, then the A record (compressed names)
        let mut r = q.clone();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 2;
        r.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xc0, 12]);
        r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 7]);
        assert_eq!(
            first_a(&r, 0x1234),
            Some(std::net::Ipv4Addr::new(203, 0, 113, 7))
        );
        assert_eq!(first_a(&r, 0x9999), None, "another question's answer");
        let mut nx = r.clone();
        nx[3] = 0x83;
        assert_eq!(first_a(&nx, 0x1234), None, "NXDOMAIN");
        assert_eq!(first_a(&r[..r.len() - 2], 0x1234), None, "cut short");
    }

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
