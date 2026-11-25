//! # ultar-tor – the internet without the device's address
//!
//! Runs the Tor client of the Tor Project in pure Rust (Arti) and offers it to the app as a
//! SOCKS5 port on the loopback device. The port takes only the app's own secret (a random
//! user name and password made at start), so no other program on the device can use it.
//! Every site gets its own circuit (as Tor Browser does): two sites cannot link the visits
//! through a shared exit. Names are resolved by Tor (`socks5h`), never by the device. The
//! port refuses addresses of the device and the local network.
//!
//! Until Tor is ready, connections wait (up to [`READY_WAIT`]); if Tor cannot start, they
//! fail: nothing ever goes out without Tor.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod socks;

use arti_client::config::TorClientConfigBuilder;
use arti_client::{IsolationToken, StreamPrefs, TorClient};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::watch;

/// Longest wait of a connection for Tor to get ready.
pub const READY_WAIT: Duration = Duration::from_secs(120);

/// Where Tor stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Connecting to the Tor network.
    Starting,
    /// Ready: connections go out.
    Ready,
    /// Tor did not start (the reason); nothing goes out.
    Failed(String),
}

/// The running Tor client and its SOCKS port.
pub struct Tor {
    port: u16,
    user: String,
    password: String,
    state: watch::Receiver<State>,
}

impl Tor {
    /// Starts Tor with its state and cache below `dir` (made if missing) and opens the port.
    pub fn start(dir: &Path) -> std::io::Result<Tor> {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let (user, password) = (secret()?, secret()?);
        let (tx, state) = watch::channel(State::Starting);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("ultar-tor")
            .build()?;
        let config = TorClientConfigBuilder::from_directories(dir.join("state"), dir.join("cache"))
            .build()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let secret = (user.clone(), password.clone());
        std::thread::Builder::new()
            .name("ultar-tor".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                        let _ = tx.send(State::Failed("port".into()));
                        return;
                    };
                    let client = Arc::new(Mutex::new(None));
                    let ready = tx.subscribe();
                    let boot = client.clone();
                    tokio::spawn(async move {
                        match TorClient::create_bootstrapped(config).await {
                            Ok(c) => {
                                *boot.lock().unwrap_or_else(PoisonError::into_inner) = Some(c);
                                let _ = tx.send(State::Ready);
                            }
                            Err(e) => {
                                let _ = tx.send(State::Failed(e.to_string()));
                            }
                        }
                    });
                    serve(listener, client, ready, secret).await;
                });
            })?;
        Ok(Tor {
            port,
            user,
            password,
            state,
        })
    }

    /// The proxy address for an HTTP client (`socks5h://user:password@127.0.0.1:port`).
    pub fn proxy_url(&self) -> String {
        format!(
            "socks5h://{}:{}@127.0.0.1:{}",
            self.user, self.password, self.port
        )
    }

    /// Where Tor stands now.
    pub fn state(&self) -> State {
        self.state.borrow().clone()
    }
}

/// A random secret of 32 hex digits.
fn secret() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

type Shared = Arc<Mutex<Option<Arc<TorClient<tor_rtcompat::PreferredRuntime>>>>>;

async fn serve(
    listener: tokio::net::TcpListener,
    client: Shared,
    ready: watch::Receiver<State>,
    secret: (String, String),
) {
    let circuits: Arc<Mutex<HashMap<String, IsolationToken>>> = Arc::default();
    loop {
        let Ok((stream, peer)) = listener.accept().await else {
            continue;
        };
        if !is_loopback(peer) {
            continue;
        }
        let (client, mut ready, secret, circuits) = (
            client.clone(),
            ready.clone(),
            secret.clone(),
            circuits.clone(),
        );
        tokio::spawn(async move {
            let _ = socks::handle(stream, &secret, |host, port| async move {
                wait_ready(&mut ready).await?;
                let tor = client
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .ok_or("tor")?;
                let mut prefs = StreamPrefs::new();
                prefs.set_isolation(circuit_of(&circuits, &host));
                tor.connect_with_prefs((host.as_str(), port), &prefs)
                    .await
                    .map_err(|_| "connect")
            })
            .await;
        });
    }
}

fn is_loopback(peer: SocketAddr) -> bool {
    peer.ip().is_loopback()
}

async fn wait_ready(ready: &mut watch::Receiver<State>) -> Result<(), &'static str> {
    let waited = tokio::time::timeout(READY_WAIT, ready.wait_for(|s| *s != State::Starting)).await;
    match waited {
        Ok(Ok(s)) if *s == State::Ready => Ok(()),
        _ => Err("tor not ready"),
    }
}

/// The circuit of a site: the registrable part of the name (`a.b.example.org` → `example.org`).
fn circuit_of(circuits: &Mutex<HashMap<String, IsolationToken>>, host: &str) -> IsolationToken {
    let site = site_of(host);
    let mut map = circuits.lock().unwrap_or_else(PoisonError::into_inner);
    *map.entry(site).or_insert_with(IsolationToken::new)
}

/// The last two labels of a host name (a simple first-party key).
pub fn site_of(host: &str) -> String {
    let labels: Vec<&str> = host.trim_end_matches('.').rsplit('.').take(2).collect();
    labels
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(".")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_and_secrets() {
        assert_eq!(site_of("ar.m.wikipedia.org"), "wikipedia.org");
        assert_eq!(site_of("Example.COM."), "example.com");
        let (a, b) = (secret().unwrap_or_default(), secret().unwrap_or_default());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
