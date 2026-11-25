//! The signalling socket `/v1/signal` (WebSocket over TLS): sign-in with the device key, then
//! messages to and from other numbers. The server writes `from`; it relays and forgets.

use crate::SignalAuth;
use serde_json::Value;
use std::net::TcpStream;
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// Time for each step of the sign-in.
const STEP: Duration = Duration::from_secs(15);
/// How long one read waits before the caller gets control back.
const POLL: Duration = Duration::from_millis(30);

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// A signed-in signalling socket.
pub struct Signal {
    ws: Socket,
    /// This device's number.
    pub number: String,
    /// The `ready` frame (it may name a pending recovery).
    pub ready: Value,
}

/// `wss://host/v1/signal` for `https://host`.
pub fn url(server_base: &str) -> String {
    let b = server_base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!("{}/v1/signal", b.trim_end_matches('/'))
}

fn set_timeout(ws: &mut Socket, t: Duration) {
    let tcp = match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::Rustls(s) => s.get_mut(),
        _ => return,
    };
    let _ = tcp.set_read_timeout(Some(t));
}

fn waiting(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

fn next_text(ws: &mut Socket) -> Result<Option<String>, String> {
    match ws.read() {
        Ok(Message::Text(t)) => Ok(Some(t.to_string())),
        Ok(Message::Close(_)) => Err("closed".into()),
        Ok(_) => Ok(None),
        Err(e) if waiting(&e) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

impl Signal {
    /// Connects and signs in; the error is a short code (`unreachable`, `auth`, …).
    pub fn connect(server_base: &str, auth: &dyn SignalAuth) -> Result<Signal, String> {
        let (mut ws, _) =
            tungstenite::connect(url(server_base)).map_err(|_| "signal_unreachable".to_string())?;
        set_timeout(&mut ws, STEP);
        ws.send(Message::text(auth.hello()?))
            .map_err(|_| "signal_unreachable")?;
        let challenge = next_text(&mut ws)?.ok_or("auth")?;
        let answer = auth.answer(&challenge)?;
        ws.send(Message::text(answer))
            .map_err(|_| "signal_unreachable")?;
        let ready: Value =
            serde_json::from_str(&next_text(&mut ws)?.ok_or("auth")?).map_err(|_| "auth")?;
        if ready.get("type").and_then(Value::as_str) != Some("ready") {
            let code = ready.get("error").and_then(Value::as_str).unwrap_or("auth");
            return Err(code.chars().take(32).collect());
        }
        let number = ready
            .get("number")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        set_timeout(&mut ws, POLL);
        Ok(Signal { ws, number, ready })
    }

    /// Sends `data` to number `to` (as one opaque `e2e` message).
    pub fn send(&mut self, to: &str, data: &str) -> Result<(), String> {
        let frame = serde_json::json!({"to": to, "type": "e2e", "data": data});
        self.ws
            .send(Message::text(frame.to_string()))
            .map_err(|e| e.to_string())
    }

    /// The next frame, if one arrives within a short moment; `Err` when the socket is gone.
    pub fn poll(&mut self) -> Result<Option<Value>, String> {
        Ok(next_text(&mut self.ws)?.and_then(|t| serde_json::from_str(&t).ok()))
    }
}
