//! `--cell-selftest`: inside the finished cell, check what the cell allows and say it on standard
//! output, one line per check (`selftest ok NAME` / `selftest FAIL NAME: why`), then end with 0
//! (all ok) or 1. Run by the integration test (`tests/cell.rs`) against the real build, so the
//! checks see exactly what Servo would see. It is the same code path as a real start up to the
//! point where Servo would be created.

use super::{view, Plan};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::time::Duration;

/// One finished check: its name and `Ok(())` or why it failed.
type Check = (&'static str, Result<(), String>);

fn expect(ok: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(why())
    }
}

fn names_of(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The first component of an absolute path (`/usr/lib` → `usr`).
fn top(path: &Path) -> String {
    path.components()
        .nth(1)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn root_holds_only_the_view(plan: &Plan) -> Result<(), String> {
    let mut allowed: Vec<String> = ["tmp", "proc", "dev"].map(String::from).to_vec();
    allowed.extend(
        plan.read_only
            .iter()
            .chain(&plan.read_write)
            .map(|p| top(p)),
    );
    let strangers: Vec<String> = names_of("/")
        .into_iter()
        .filter(|n| !allowed.contains(n))
        .collect();
    expect(strangers.is_empty(), || {
        format!("unexpected in /: {strangers:?}")
    })
}

fn only_own_processes() -> Result<(), String> {
    let pids: Vec<String> = names_of("/proc")
        .into_iter()
        .filter(|n| n.bytes().all(|b| b.is_ascii_digit()))
        .collect();
    expect(pids == ["1"], || format!("processes visible: {pids:?}"))
}

fn status_field(name: &str) -> String {
    ultar_sys::cellnet::own_status_field(name).unwrap_or_default()
}

fn no_privileges() -> Result<(), String> {
    let (eff, prm, nnp) = (
        status_field("CapEff:"),
        status_field("CapPrm:"),
        status_field("NoNewPrivs:"),
    );
    let zero = |s: &str| !s.is_empty() && s.bytes().all(|b| b == b'0');
    expect(zero(&eff) && zero(&prm) && nnp == "1", || {
        format!("CapEff={eff} CapPrm={prm} NoNewPrivs={nnp}")
    })
}

fn only_loopback_device() -> Result<(), String> {
    let devices = ultar_sys::cellnet::network_devices();
    expect(devices == ["lo"], || {
        format!("network devices: {devices:?}")
    })
}

fn no_way_out(address: &str) -> Result<(), String> {
    let target: SocketAddr = address.parse().map_err(|_| "address".to_string())?;
    let tcp = TcpStream::connect_timeout(&target, Duration::from_secs(3));
    let udp = UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect(target));
    expect(tcp.is_err() && udp.is_err(), || {
        format!("{address}: tcp {:?} udp {:?}", tcp.is_ok(), udp.is_ok())
    })
}

/// Opening a socket to anything but the loopback fails, also over IPv6.
fn no_ipv6_way_out() -> Result<(), String> {
    let target: SocketAddr = "[2606:4700:4700::1111]:443"
        .parse()
        .map_err(|_| "address")?;
    let tcp = TcpStream::connect_timeout(&target, Duration::from_secs(3));
    expect(tcp.is_err(), || "reached an IPv6 address".to_string())
}

fn folders_are_as_declared(plan: &Plan) -> Result<(), String> {
    let write = |dir: &Path| std::fs::write(dir.join("selftest-probe"), b"x");
    let mut problems = Vec::new();
    for dir in &plan.read_write {
        match write(dir) {
            Ok(()) => {
                let _ = std::fs::remove_file(dir.join("selftest-probe"));
            }
            Err(e) => problems.push(format!("{} not writable: {e}", dir.display())),
        }
    }
    for dir in plan.read_only.iter().filter(|d| d.is_dir()) {
        if write(dir).is_ok() {
            let _ = std::fs::remove_file(dir.join("selftest-probe"));
            problems.push(format!("{} is writable", dir.display()));
        }
    }
    if std::fs::create_dir("/selftest-root").is_ok() {
        problems.push("the root is writable".into());
    }
    expect(problems.is_empty(), || problems.join("; "))
}

fn tmp_is_private_and_writable(plan: &Plan) -> Result<(), String> {
    let probe = Path::new(view::TMP).join("selftest");
    std::fs::write(&probe, b"x").map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(probe);
    // the mount points of cell folders that lie below /tmp are the only other entries
    let mounted: Vec<String> = plan
        .read_only
        .iter()
        .chain(&plan.read_write)
        .filter_map(|p| p.strip_prefix(view::TMP).ok())
        .filter_map(|p| p.components().next())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let others: Vec<String> = names_of(view::TMP)
        .into_iter()
        .filter(|n| !mounted.contains(n))
        .collect();
    expect(others.is_empty(), || {
        format!("/tmp was not empty: {others:?}")
    })
}

fn devices_are_the_harmless_ones() -> Result<(), String> {
    let allowed = view::device_names();
    let extra: Vec<String> = names_of("/dev")
        .into_iter()
        .filter(|n| !allowed.contains(&n.as_str()))
        .collect();
    expect(extra.is_empty(), || {
        format!("unexpected in /dev: {extra:?}")
    })
}

/// A `CONNECT` through the relay reaches the browser's socket: its answer starts with `200` and
/// bytes written afterwards come back (the test's socket server echoes).
fn relay_reaches_the_socket(port: u16) -> Result<(), String> {
    let mut c = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    c.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    c.write_all(b"CONNECT selftest.invalid:443 HTTP/1.1\r\n\r\n")
        .map_err(|e| e.to_string())?;
    let mut head = [0u8; 39];
    c.read_exact(&mut head).map_err(|e| e.to_string())?;
    let head = String::from_utf8_lossy(&head).into_owned();
    expect(head.starts_with("HTTP/1.1 200"), || {
        format!("answer: {head:?}")
    })?;
    c.write_all(b"ping").map_err(|e| e.to_string())?;
    let mut back = [0u8; 4];
    c.read_exact(&mut back).map_err(|e| e.to_string())?;
    expect(&back == b"ping", || format!("echo: {back:?}"))
}

/// Runs every check, prints the lines and returns the exit code (0: all ok).
pub fn run(plan: &Plan, relay_port: u16) -> u8 {
    let checks: Vec<Check> = vec![
        (
            "no-home",
            expect(
                !Path::new("/home").exists() && !Path::new("/root").exists(),
                || "/home or /root visible".into(),
            ),
        ),
        ("root-holds-only-the-view", root_holds_only_the_view(plan)),
        ("only-own-processes", only_own_processes()),
        ("no-capabilities-no-new-privileges", no_privileges()),
        ("only-the-loopback-device", only_loopback_device()),
        ("no-route-to-1.1.1.1", no_way_out("1.1.1.1:443")),
        ("no-route-to-the-lan", no_way_out("192.168.100.1:80")),
        ("no-route-to-the-lan-10", no_way_out("10.0.0.1:80")),
        ("no-route-by-ipv6", no_ipv6_way_out()),
        ("folders-as-declared", folders_are_as_declared(plan)),
        (
            "tmp-private-and-writable",
            tmp_is_private_and_writable(plan),
        ),
        ("only-harmless-devices", devices_are_the_harmless_ones()),
        (
            "relay-reaches-the-browser-socket",
            relay_reaches_the_socket(relay_port),
        ),
    ];
    let mut failed = 0u8;
    let mut out = std::io::stdout();
    for (name, result) in checks {
        let _ = match result {
            Ok(()) => writeln!(out, "selftest ok {name}"),
            Err(why) => {
                failed = failed.saturating_add(1);
                writeln!(out, "selftest FAIL {name}: {why}")
            }
        };
    }
    let _ = out.flush();
    u8::from(failed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_read_the_system_as_expected() {
        assert_eq!(top(Path::new("/usr/lib/ultar")), "usr");
        assert_eq!(top(Path::new("/")), "");
        assert!(expect(true, String::new).is_ok());
        assert_eq!(expect(false, || "no".into()), Err("no".into()));
        assert!(names_of("/nonexistent-ultar-dir").is_empty());
    }
}
