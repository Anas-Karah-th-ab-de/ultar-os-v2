//! The accept loop: every limit of the process towards slow or numerous clients is set here.
//!
//! * A connection must send the head of its request within [`HEADER_READ_TIMEOUT`], or it is
//!   closed. (The whole request, body included, has [`crate::guard::REQUEST_TIMEOUT`].)
//! * At most `max_connections` connections are open, at most `max_connections_per_peer` per
//!   client; when full, the oldest idle connection gives way ([`crate::conntable`]).
//!
//! The reverse proxy in front has limits of its own; these hold without it.

use crate::conntable::{BusyGuard, ConnTable, Slot};
use crate::AppState;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::Extension;
use hyper::body::Incoming;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::net::TcpStream;
use tower::ServiceExt;

/// Time a client has to send a complete request head.
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections open at the same time when not configured.
pub const DEFAULT_MAX_CONNECTIONS: usize = 4096;
/// Connections of one client (IPv4 address or IPv6 /64) when not configured.
pub const DEFAULT_MAX_CONNECTIONS_PER_PEER: usize = 64;
/// Pause after a failed `accept` (for example: no file descriptors left).
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// Parses the connection cap (16 to 1 000 000).
pub fn parse_max_connections(text: &str) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|n| (16..=1_000_000).contains(n))
}

/// Parses the per-client connection cap (1 to 1 000 000).
pub fn parse_max_connections_per_peer(text: &str) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=1_000_000).contains(n))
}

/// A response body that keeps its connection marked busy until it is sent or dropped.
struct TrackedBody {
    inner: Body,
    _busy: BusyGuard,
}

impl http_body::Body for TrackedBody {
    type Data = axum::body::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

async fn connection(stream: TcpStream, peer: SocketAddr, app: axum::Router, slot: Slot) {
    let app = app.layer(Extension(ConnectInfo(peer)));
    let busy = slot.busy.clone();
    let service = tower::service_fn(move |request: hyper::Request<Incoming>| {
        let (app, guard) = (app.clone(), BusyGuard::new(busy.clone()));
        async move {
            let response = app.oneshot(request).await?;
            let tracked = response.map(|inner| {
                Body::new(TrackedBody {
                    inner,
                    _busy: guard,
                })
            });
            Ok::<_, Infallible>(tracked)
        }
    });
    let mut http = hyper::server::conn::http1::Builder::new();
    http.timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
    let served = http
        .serve_connection(TokioIo::new(stream), TowerToHyperService::new(service))
        .with_upgrades();
    // Errors are the client's (reset, timeout, malformed request); there is nothing to do.
    // Giving way drops the connection, which closes it.
    tokio::select! {
        _ = served => {}
        () = slot.evict.notified() => {}
    }
}

/// Serves the router on an already bound listener until `shutdown` resolves.
pub async fn serve<F>(
    listener: tokio::net::TcpListener,
    state: Arc<AppState>,
    shutdown: F,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let table = Arc::new(ConnTable::new(
        state.max_connections,
        state.max_connections_per_peer,
        state.trusted_proxies.clone(),
    ));
    let app = crate::router(state);
    tokio::pin!(shutdown);
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            () = &mut shutdown => return Ok(()),
        };
        match accepted {
            Ok((stream, peer)) => match table.admit(peer.ip()) {
                Some(slot) => {
                    tokio::spawn(connection(stream, peer, app.clone(), slot));
                }
                // Every connection within the bound is busy: this one is closed at once.
                None => drop(stream),
            },
            Err(e) => {
                tracing::warn!(event = "accept_failed", error = %e);
                tokio::time::sleep(ACCEPT_BACKOFF).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_caps_from_text() {
        assert_eq!(parse_max_connections("4096"), Some(4096));
        assert_eq!(parse_max_connections(" 16 "), Some(16));
        for bad in ["15", "0", "-1", "x", "1000001"] {
            assert_eq!(parse_max_connections(bad), None, "{bad}");
        }
        assert_eq!(parse_max_connections_per_peer("1"), Some(1));
        for bad in ["0", "x", "1000001"] {
            assert_eq!(parse_max_connections_per_peer(bad), None, "{bad}");
        }
    }
}
