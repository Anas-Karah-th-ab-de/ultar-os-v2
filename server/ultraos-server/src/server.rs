//! The accept loop: every limit of the process towards slow or numerous clients is set here.
//!
//! * A connection must send the head of its request within [`HEADER_READ_TIMEOUT`], or it is
//!   closed. (The whole request, body included, has [`crate::guard::REQUEST_TIMEOUT`].)
//! * At most `max_connections` connections are open at the same time; further clients wait in
//!   the kernel's accept queue.
//!
//! The reverse proxy in front has limits of its own; these hold without it.

use crate::AppState;
use axum::extract::ConnectInfo;
use axum::Extension;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Time a client has to send a complete request head.
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections open at the same time when not configured.
pub const DEFAULT_MAX_CONNECTIONS: usize = 4096;
/// Pause after a failed `accept` (for example: no file descriptors left).
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// Parses the connection cap (16 to 1 000 000).
pub fn parse_max_connections(text: &str) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|n| (16..=1_000_000).contains(n))
}

async fn connection(
    stream: TcpStream,
    peer: SocketAddr,
    app: axum::Router,
    permit: OwnedSemaphorePermit,
) {
    let service = TowerToHyperService::new(app.layer(Extension(ConnectInfo(peer))));
    let mut http = hyper::server::conn::http1::Builder::new();
    http.timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
    // Errors here are the client's (reset, timeout, malformed request); there is nothing to do.
    let _ = http
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades()
        .await;
    drop(permit);
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
    let slots = Arc::new(Semaphore::new(state.max_connections.max(1)));
    let app = crate::router(state);
    tokio::pin!(shutdown);
    loop {
        let permit = tokio::select! {
            permit = slots.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => return Ok(()),
            },
            () = &mut shutdown => return Ok(()),
        };
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            () = &mut shutdown => return Ok(()),
        };
        match accepted {
            Ok((stream, peer)) => {
                tokio::spawn(connection(stream, peer, app.clone(), permit));
            }
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
    }
}
