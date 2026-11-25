//! The SOCKS5 side of the port (RFC 1928 with user name and password, RFC 1929): CONNECT
//! only, names passed on unresolved; addresses of this device and the local network are
//! refused before Tor is asked.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const VERSION: u8 = 5;
const USER_PASS: u8 = 2;
const NO_METHOD: u8 = 0xFF;
const CONNECT: u8 = 1;
/// Reply codes.
const OK: u8 = 0;
const FAILURE: u8 = 1;
const NOT_ALLOWED: u8 = 2;
const COMMAND: u8 = 7;
const ADDRESS: u8 = 8;

/// Serves one SOCKS connection: checks the secret, reads the target, connects with
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
    greet(&mut client, secret).await?;
    let (host, port) = match target(&mut client).await {
        Ok(t) => t,
        Err(code) => return reply(&mut client, code).await,
    };
    if local(&host) {
        return reply(&mut client, NOT_ALLOWED).await;
    }
    let mut remote = match connect(host, port).await {
        Ok(r) => r,
        Err(_) => return reply(&mut client, FAILURE).await,
    };
    reply(&mut client, OK).await?;
    tokio::io::copy_bidirectional(&mut client, &mut remote)
        .await
        .map(|_| ())
        .map_err(|_| "copy")
}

async fn greet<C: AsyncRead + AsyncWrite + Unpin>(
    c: &mut C,
    secret: &(String, String),
) -> Result<(), &'static str> {
    let io = |_| "io";
    let mut head = [0u8; 2];
    c.read_exact(&mut head).await.map_err(io)?;
    let mut methods = vec![0u8; usize::from(head[1])];
    c.read_exact(&mut methods).await.map_err(io)?;
    if head[0] != VERSION || !methods.contains(&USER_PASS) {
        let _ = c.write_all(&[VERSION, NO_METHOD]).await;
        return Err("method");
    }
    c.write_all(&[VERSION, USER_PASS]).await.map_err(io)?;
    let mut ver = [0u8; 2];
    c.read_exact(&mut ver).await.map_err(io)?;
    let mut user = vec![0u8; usize::from(ver[1])];
    c.read_exact(&mut user).await.map_err(io)?;
    let mut len = [0u8; 1];
    c.read_exact(&mut len).await.map_err(io)?;
    let mut pass = vec![0u8; usize::from(len[0])];
    c.read_exact(&mut pass).await.map_err(io)?;
    let good = same(&user, secret.0.as_bytes()) & same(&pass, secret.1.as_bytes());
    c.write_all(&[1, if good { 0 } else { 1 }])
        .await
        .map_err(io)?;
    if good {
        Ok(())
    } else {
        Err("secret")
    }
}

/// Compares in constant time for equal lengths.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

async fn target<C: AsyncRead + Unpin>(c: &mut C) -> Result<(String, u16), u8> {
    let mut head = [0u8; 4];
    c.read_exact(&mut head).await.map_err(|_| FAILURE)?;
    if head[0] != VERSION || head[1] != CONNECT {
        return Err(COMMAND);
    }
    let host = match head[3] {
        1 => {
            let mut a = [0u8; 4];
            c.read_exact(&mut a).await.map_err(|_| FAILURE)?;
            Ipv4Addr::from(a).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            c.read_exact(&mut len).await.map_err(|_| FAILURE)?;
            let mut name = vec![0u8; usize::from(len[0])];
            c.read_exact(&mut name).await.map_err(|_| FAILURE)?;
            String::from_utf8(name).map_err(|_| ADDRESS)?
        }
        4 => {
            let mut a = [0u8; 16];
            c.read_exact(&mut a).await.map_err(|_| FAILURE)?;
            Ipv6Addr::from(a).to_string()
        }
        _ => return Err(ADDRESS),
    };
    let mut port = [0u8; 2];
    c.read_exact(&mut port).await.map_err(|_| FAILURE)?;
    Ok((host, u16::from_be_bytes(port)))
}

async fn reply<C: AsyncWrite + Unpin>(c: &mut C, code: u8) -> Result<(), &'static str> {
    c.write_all(&[VERSION, code, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(|_| "io")?;
    if code == OK {
        Ok(())
    } else {
        Err("refused")
    }
}

/// This device or the local network, by name or address.
fn local(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || !h.contains('.') && !h.contains(':')
    {
        return true;
    }
    match h.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
        }
        Ok(IpAddr::V6(v6)) => {
            let s = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (s & 0xfe00) == 0xfc00
                || (s & 0xffc0) == 0xfe80
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_loopback() || v4.is_private())
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn secret() -> (String, String) {
        ("u".into(), "p".into())
    }

    async fn client_side(mut c: tokio::io::DuplexStream, target: &[u8], pass: &[u8]) -> Vec<u8> {
        c.write_all(&[5, 1, 2]).await.unwrap_or_default();
        let mut m = [0u8; 2];
        c.read_exact(&mut m).await.unwrap_or_default();
        let mut auth = vec![1, 1, b'u', pass.len() as u8];
        auth.extend_from_slice(pass);
        c.write_all(&auth).await.unwrap_or_default();
        let mut a = [0u8; 2];
        c.read_exact(&mut a).await.unwrap_or_default();
        if a[1] != 0 {
            return vec![0xEE];
        }
        c.write_all(target).await.unwrap_or_default();
        let mut r = [0u8; 10];
        c.read_exact(&mut r).await.unwrap_or_default();
        if r[1] != 0 {
            return vec![r[1]];
        }
        c.write_all(b"ping").await.unwrap_or_default();
        let mut back = [0u8; 4];
        c.read_exact(&mut back).await.unwrap_or_default();
        back.to_vec()
    }

    fn domain(name: &str, port: u16) -> Vec<u8> {
        let mut t = vec![5, 1, 0, 3, name.len() as u8];
        t.extend_from_slice(name.as_bytes());
        t.extend_from_slice(&port.to_be_bytes());
        t
    }

    async fn run(target: Vec<u8>, pass: &'static [u8]) -> (Vec<u8>, Option<(String, u16)>) {
        let (a, b) = duplex(1024);
        let asked = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen = asked.clone();
        let server = tokio::spawn(async move {
            let _ = handle(b, &secret(), |host, port| async move {
                *seen
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((host, port));
                let (x, mut y) = duplex(64);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4];
                    if y.read_exact(&mut buf).await.is_ok() {
                        let _ = y.write_all(b"pong").await;
                    }
                });
                Ok(x)
            })
            .await;
        });
        let got = client_side(a, &target, pass).await;
        let _ = server.await;
        let asked = asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        (got, asked)
    }

    #[tokio::test]
    async fn a_name_is_passed_on_and_bytes_flow() {
        let (got, asked) = run(domain("example.org", 443), b"p").await;
        assert_eq!(got, b"pong");
        assert_eq!(asked, Some(("example.org".into(), 443)));
    }

    #[tokio::test]
    async fn the_secret_and_local_addresses_are_checked() {
        assert_eq!(run(domain("example.org", 443), b"x").await.0, [0xEE]);
        assert_eq!(
            run(domain("localhost", 80), b"p").await,
            (vec![NOT_ALLOWED], None)
        );
        assert_eq!(
            run(vec![5, 1, 0, 1, 192, 168, 1, 1, 0, 80], b"p").await,
            (vec![NOT_ALLOWED], None)
        );
        assert_eq!(
            run(vec![5, 2, 0, 1, 8, 8, 8, 8, 0, 80], b"p").await.0,
            [COMMAND]
        );
    }
}

#[cfg(test)]
mod with_http_client {
    use super::*;

    /// An HTTP client (ureq, as the browser uses it) through the port: the secret in the proxy
    /// address, the name passed on unresolved.
    #[test]
    fn an_http_client_goes_through_the_port() {
        let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| panic!("{e}"));
        let site = std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
        let site_addr = site.local_addr().unwrap_or_else(|e| panic!("{e}"));
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            if let Ok((mut s, _)) = site.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                );
            }
        });
        let port = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap_or_else(|e| panic!("{e}"));
            let port = listener
                .local_addr()
                .unwrap_or_else(|e| panic!("{e}"))
                .port();
            tokio::spawn(async move {
                if let Ok((stream, _)) = listener.accept().await {
                    let secret = ("user1".to_owned(), "pass1".to_owned());
                    let _ = handle(stream, &secret, |host, _port| async move {
                        // "the internet": the name reaches the connector unresolved
                        if host != "site.example" {
                            return Err("name");
                        }
                        tokio::net::TcpStream::connect(site_addr)
                            .await
                            .map_err(|_| "connect")
                    })
                    .await;
                }
            });
            port
        });
        let proxy = ureq::Proxy::new(&format!("socks5h://user1:pass1@127.0.0.1:{port}"))
            .unwrap_or_else(|e| panic!("{e}"));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(Some(proxy))
            .build()
            .into();
        let body = agent
            .get("http://site.example/")
            .call()
            .unwrap_or_else(|e| panic!("{e}"))
            .into_body()
            .read_to_string()
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body, "hello");
    }
}
