//! Whether the internet can be reached: a worker thread looks up the check host through the
//! name servers in use and opens a TCP connection to it, after every change of the network
//! and every minute. No address: offline; an address but no answer: local network only.

use crate::dns;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use ultar_bus::messages::net::Internet;

/// The default check address: the update server of Ultar OS.
pub const DEFAULT_TARGET: &str = "ultar.code-craft.tech:443";
/// How often a connected network is checked again.
pub const EVERY: Duration = Duration::from_secs(60);
const WAIT: Duration = Duration::from_secs(4);

/// Host and port to check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Name or address.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

impl Target {
    /// `host:port` (`None`: not of that form).
    pub fn parse(text: &str) -> Option<Target> {
        let (host, port) = text.rsplit_once(':')?;
        let port = port.parse().ok().filter(|p| *p > 0)?;
        (!host.is_empty()).then(|| Target {
            host: host.to_owned(),
            port,
        })
    }
}

/// What the check needs: the name servers of the network in use (`None`: no address).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    /// Name servers.
    pub dns: Vec<String>,
    /// Default router (part of the change: another network is checked again).
    pub gateway: Option<String>,
}

#[derive(Default)]
struct Shared {
    wanted: Option<Network>,
    changed: bool,
    result: Internet,
    stop: bool,
}

/// The check worker.
pub struct Prober {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Prober {
    /// Starts the worker (none when the thread cannot be started: the state stays "checking").
    pub fn start(target: Target) -> Prober {
        let shared: Arc<(Mutex<Shared>, Condvar)> = Arc::default();
        let inner = shared.clone();
        let worker = std::thread::Builder::new()
            .name("netd-internet".into())
            .spawn(move || run(&inner, &target))
            .ok();
        Prober { shared, worker }
    }

    /// The network in use now; a change starts a check at once.
    pub fn network(&self, now: Option<Network>) {
        let (lock, cv) = &*self.shared;
        let Ok(mut s) = lock.lock() else { return };
        if s.wanted != now {
            s.result = if now.is_some() {
                Internet::Checking
            } else {
                Internet::Offline
            };
            s.wanted = now;
            s.changed = true;
            cv.notify_all();
        }
    }

    /// The last result.
    pub fn result(&self) -> Internet {
        self.shared
            .0
            .lock()
            .map(|s| s.result)
            .unwrap_or(Internet::Checking)
    }
}

impl Drop for Prober {
    fn drop(&mut self) {
        if let Ok(mut s) = self.shared.0.lock() {
            s.stop = true;
        }
        self.shared.1.notify_all();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

fn run(shared: &(Mutex<Shared>, Condvar), target: &Target) {
    let (lock, cv) = shared;
    loop {
        let wanted = {
            let Ok(mut s) = lock.lock() else { return };
            if !s.changed && !s.stop {
                s = match cv.wait_timeout(s, EVERY) {
                    Ok((s, _)) => s,
                    Err(_) => return,
                };
            }
            if s.stop {
                return;
            }
            s.changed = false;
            s.wanted.clone()
        };
        let Some(net) = wanted else { continue };
        let result = if reachable(target, &net.dns) {
            Internet::Connected
        } else {
            Internet::LocalOnly
        };
        let Ok(mut s) = lock.lock() else { return };
        // a change while checking: that network is checked next, this answer is old
        if !s.changed && s.wanted.as_ref() == Some(&net) {
            s.result = result;
        }
    }
}

/// The check: resolve (unless an address), then a TCP connection.
pub fn reachable(target: &Target, servers: &[String]) -> bool {
    let ip = match target.host.parse::<IpAddr>() {
        Ok(ip) => Some(ip),
        Err(_) => servers
            .iter()
            .filter_map(|s| s.parse::<Ipv4Addr>().ok())
            .find_map(|s| resolve(s, &target.host))
            .map(IpAddr::V4),
    };
    ip.is_some_and(|ip| TcpStream::connect_timeout(&SocketAddr::new(ip, target.port), WAIT).is_ok())
}

fn resolve(server: Ipv4Addr, name: &str) -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    socket.connect((server, 53)).ok()?;
    let mut id = [0u8; 2];
    getrandom::getrandom(&mut id).ok()?;
    let id = u16::from_be_bytes(id);
    let q = dns::query(name, id)?;
    let mut buf = [0u8; 1500];
    for _ in 0..2 {
        socket.send(&q).ok()?;
        // answers to other questions are skipped until the time is up
        while let Ok(n) = socket.recv(&mut buf) {
            if let Some(a) = dns::first_a(&buf[..n], id) {
                return Some(a);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(
            Target::parse(DEFAULT_TARGET),
            Some(Target {
                host: "ultar.code-craft.tech".into(),
                port: 443
            })
        );
        assert_eq!(Target::parse("10.0.2.2:80").unwrap().port, 80);
        assert!(Target::parse("host").is_none() && Target::parse(":80").is_none());
        assert!(Target::parse("host:0").is_none());
    }

    #[test]
    fn local_listener_is_reached_and_a_closed_port_is_not() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = Target {
            host: "127.0.0.1".into(),
            port,
        };
        assert!(reachable(&t, &[]));
        drop(l);
        assert!(!reachable(&t, &[]));
        let named = Target {
            host: "name.invalid".into(),
            port,
        };
        assert!(!reachable(&named, &[]), "no name server: no address");
    }

    #[test]
    fn the_worker_reports_offline_checking_and_the_answer() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let t = Target {
            host: "127.0.0.1".into(),
            port: l.local_addr().unwrap().port(),
        };
        let p = Prober::start(t);
        assert_eq!(p.result(), Internet::Offline);
        p.network(Some(Network {
            dns: vec![],
            gateway: None,
        }));
        let end = std::time::Instant::now() + Duration::from_secs(10);
        while p.result() != Internet::Connected && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(p.result(), Internet::Connected);
        p.network(None);
        assert_eq!(p.result(), Internet::Offline);
    }
}
