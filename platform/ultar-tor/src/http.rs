//! The HTTP side of the port (RFC 9110 §9.3.6, `CONNECT` only) for engines that reach the
//! internet through an HTTP proxy (the Servo engine of the browser). The app's secret comes as
//! `Proxy-Authorization: Basic …`; the same refusals as the SOCKS side apply (this device, the
//! local network); after `200` the bytes are copied both ways. TLS stays between the engine and
//! the site: the port sees only the name and the port of the site.

use std::future::Future;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Longest request head read from the engine.
const MAX_HEAD: usize = 8 * 1024;

/// Serves one proxy connection: checks the secret, reads the `CONNECT` target, connects with
/// `connect(host, port)` and copies both ways until one side ends.
pub async fn handle<C, F, Fut, S>(
    mut client: C,
    secret: &(String, String),
    connect: F,
) -> Result<(), &'static str>
where
    C: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(String, u16) -> Fut,
    Fut: Future<Output = Result<S, &'static str>>,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (head, extra) = read_head(&mut client).await?;
    let Some(request) = Request::parse(&head) else {
        return refuse(&mut client, "400 Bad Request", "").await;
    };
    if !request.carries(&basic_token(secret)) {
        let challenge = "Proxy-Authenticate: Basic realm=\"ultar\"\r\n";
        return refuse(&mut client, "407 Proxy Authentication Required", challenge).await;
    }
    if !request.method.eq_ignore_ascii_case("CONNECT") {
        return refuse(&mut client, "405 Method Not Allowed", "").await;
    }
    let Some((host, port)) = authority(request.target) else {
        return refuse(&mut client, "400 Bad Request", "").await;
    };
    if crate::socks::local(&host) {
        return refuse(&mut client, "403 Forbidden", "").await;
    }
    let mut remote = match connect(host, port).await {
        Ok(r) => r,
        Err(_) => return refuse(&mut client, "502 Bad Gateway", "").await,
    };
    client
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await
        .map_err(|_| "io")?;
    if !extra.is_empty() {
        remote.write_all(&extra).await.map_err(|_| "io")?;
    }
    tokio::io::copy_bidirectional(&mut client, &mut remote)
        .await
        .map(|_| ())
        .map_err(|_| "copy")
}

/// The request head (up to the empty line) and the bytes that came after it.
async fn read_head<C: AsyncRead + Unpin>(c: &mut C) -> Result<(String, Vec<u8>), &'static str> {
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    loop {
        let n = c.read(&mut chunk).await.map_err(|_| "io")?;
        if n == 0 {
            return Err("closed");
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let extra = buf.split_off(end + 4);
            let head = String::from_utf8(buf).map_err(|_| "head")?;
            return Ok((head, extra));
        }
        if buf.len() > MAX_HEAD {
            return Err("head");
        }
    }
}

async fn refuse<C: AsyncWrite + Unpin>(
    c: &mut C,
    status: &str,
    extra_header: &str,
) -> Result<(), &'static str> {
    let answer = format!(
        "HTTP/1.1 {status}\r\n{extra_header}Content-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let _ = c.write_all(answer.as_bytes()).await;
    Err("refused")
}

/// The parts of a request head the port looks at.
struct Request<'a> {
    method: &'a str,
    target: &'a str,
    authorization: Option<&'a str>,
}

impl<'a> Request<'a> {
    fn parse(head: &'a str) -> Option<Request<'a>> {
        let mut lines = head.split("\r\n");
        let mut first = lines.next()?.split(' ');
        let (method, target, version) = (first.next()?, first.next()?, first.next()?);
        if !version.starts_with("HTTP/1.") || first.next().is_some() {
            return None;
        }
        let authorization = lines
            .filter_map(|l| l.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case("proxy-authorization"))
            .map(|(_, value)| value.trim());
        Some(Request {
            method,
            target,
            authorization,
        })
    }

    /// Whether the request carries `Basic <token>` (compared in constant time).
    fn carries(&self, token: &str) -> bool {
        let Some((scheme, value)) = self.authorization.and_then(|v| v.split_once(' ')) else {
            return false;
        };
        scheme.eq_ignore_ascii_case("basic") && same(value.trim().as_bytes(), token.as_bytes())
    }
}

/// Compares in constant time for equal lengths.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// `host:port` or `[v6]:port` with a port that is not 0.
fn authority(target: &str) -> Option<(String, u16)> {
    let (host, port) = match target.strip_prefix('[') {
        Some(v6) => {
            let (host, rest) = v6.split_once(']')?;
            (host, rest.strip_prefix(':')?)
        }
        None => target.rsplit_once(':')?,
    };
    let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
    (!host.is_empty() && !host.contains(['/', '@', ' '])).then(|| (host.to_string(), port))
}

/// `base64(user ":" password)` – what a client sends after `Basic`.
pub(crate) fn basic_token(secret: &(String, String)) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let plain = format!("{}:{}", secret.0, secret.1);
    let mut out = String::with_capacity(plain.len().div_ceil(3) * 4);
    for group in plain.as_bytes().chunks(3) {
        let b = [
            group[0],
            group.get(1).copied().unwrap_or(0),
            group.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= group.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn secret() -> (String, String) {
        ("u".into(), "p".into())
    }

    async fn ask(head: &str) -> String {
        let (mut ours, theirs) = duplex(4096);
        let server = tokio::spawn(async move {
            let _ = handle(theirs, &secret(), |_host, _port| async {
                let (a, mut b) = duplex(64);
                tokio::spawn(async move {
                    let mut x = [0u8; 4];
                    if b.read_exact(&mut x).await.is_ok() {
                        let _ = b.write_all(&x).await;
                    }
                });
                Ok(a)
            })
            .await;
        });
        ours.write_all(head.as_bytes()).await.unwrap_or_default();
        let mut answer = vec![0u8; 39];
        let n = ours.read(&mut answer).await.unwrap_or(0);
        let mut text = String::from_utf8_lossy(&answer[..n]).into_owned();
        if text.starts_with("HTTP/1.1 200") {
            ours.write_all(b"ping").await.unwrap_or_default();
            let mut back = [0u8; 4];
            ours.read_exact(&mut back).await.unwrap_or_default();
            text.push_str(&String::from_utf8_lossy(&back));
        }
        drop(ours);
        let _ = server.await;
        text
    }

    #[test]
    fn token_is_base64_of_user_and_password() {
        assert_eq!(basic_token(&secret()), "dTpw");
        assert_eq!(
            basic_token(&("Aladdin".into(), "open sesame".into())),
            "QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[tokio::test]
    async fn connect_with_the_secret_goes_through() {
        let a = ask("CONNECT example.org:443 HTTP/1.1\r\nHost: example.org:443\r\nProxy-Authorization: Basic dTpw\r\n\r\n").await;
        assert!(a.starts_with("HTTP/1.1 200"), "{a}");
        assert!(a.ends_with("ping"), "{a}");
    }

    #[tokio::test]
    async fn refusals() {
        let no_secret = ask("CONNECT example.org:443 HTTP/1.1\r\n\r\n").await;
        assert!(no_secret.starts_with("HTTP/1.1 407"), "{no_secret}");
        let wrong =
            ask("CONNECT example.org:443 HTTP/1.1\r\nProxy-Authorization: Basic dTpx\r\n\r\n")
                .await;
        assert!(wrong.starts_with("HTTP/1.1 407"), "{wrong}");
        let local =
            ask("CONNECT 127.0.0.1:22 HTTP/1.1\r\nProxy-Authorization: basic dTpw\r\n\r\n").await;
        assert!(local.starts_with("HTTP/1.1 403"), "{local}");
        let lan =
            ask("CONNECT [fe80::1]:80 HTTP/1.1\r\nProxy-Authorization: Basic dTpw\r\n\r\n").await;
        assert!(lan.starts_with("HTTP/1.1 403"), "{lan}");
        let get =
            ask("GET http://example.org/ HTTP/1.1\r\nProxy-Authorization: Basic dTpw\r\n\r\n")
                .await;
        assert!(get.starts_with("HTTP/1.1 405"), "{get}");
    }

    #[test]
    fn authorities() {
        assert_eq!(authority("a.org:443"), Some(("a.org".into(), 443)));
        assert_eq!(
            authority("[2606:4700::1111]:443"),
            Some(("2606:4700::1111".into(), 443))
        );
        assert_eq!(authority("a.org"), None);
        assert_eq!(authority("a.org:0"), None);
        assert_eq!(authority("u@a.org:443"), None);
    }
}
