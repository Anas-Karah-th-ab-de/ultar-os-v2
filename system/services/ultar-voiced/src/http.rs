//! The smallest HTTP/1.1 client the service needs: one multipart POST to the speech-to-text
//! server on the loopback address, answered with a status and a body. No TLS, no redirects,
//! no keep-alive: the server only ever lives on 127.0.0.1.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Largest answer read (the transcript is a few lines).
const MAX_ANSWER: usize = 1 << 20;

/// Whether something accepts connections at `addr`.
pub fn can_connect(addr: &SocketAddr, timeout: Duration) -> bool {
    TcpStream::connect_timeout(addr, timeout).is_ok()
}

/// A multipart form: text fields and one file.
pub struct Form<'a> {
    /// Text fields (name, value).
    pub fields: &'a [(&'a str, &'a str)],
    /// The file field name.
    pub file_field: &'a str,
    /// The file name shown to the server.
    pub file_name: &'a str,
    /// The file content.
    pub file: &'a [u8],
}

fn body(form: &Form<'_>, boundary: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(form.file.len() + 512);
    for (name, value) in form.fields {
        out.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n\
             Content-Type: audio/wav\r\n\r\n",
            form.file_field, form.file_name
        )
        .as_bytes(),
    );
    out.extend_from_slice(form.file);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// Posts the form to `path` and returns the status code and the body.
pub fn post_form(
    addr: &SocketAddr,
    path: &str,
    form: &Form<'_>,
    timeout: Duration,
) -> io::Result<(u16, Vec<u8>)> {
    let boundary = format!("----ultar-voiced-{}", std::process::id());
    let payload = body(form, &boundary);
    let mut stream = TcpStream::connect_timeout(addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: multipart/form-data; \
         boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&payload)?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
        if raw.len() > MAX_ANSWER {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "answer too long",
            ));
        }
    }
    parse_answer(&raw)
}

fn bad(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Status and body of a raw HTTP/1.1 answer (also a chunked one).
pub fn parse_answer(raw: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    let end = find(raw, b"\r\n\r\n").ok_or_else(|| bad("no header"))?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| bad("header"))?;
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| bad("status"))?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let rest = &raw[end + 4..];
    if !chunked {
        return Ok((status, rest.to_vec()));
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        let line_end = find(&rest[at..], b"\r\n").ok_or_else(|| bad("chunk"))? + at;
        let size_text = std::str::from_utf8(&rest[at..line_end]).map_err(|_| bad("chunk"))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| bad("chunk size"))?;
        at = line_end + 2;
        if size == 0 {
            return Ok((status, out));
        }
        let stop = at.checked_add(size).ok_or_else(|| bad("chunk"))?;
        out.extend_from_slice(rest.get(at..stop).ok_or_else(|| bad("cut chunk"))?);
        at = stop + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn plain_and_chunked_answers_are_read() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody";
        assert_eq!(parse_answer(plain).unwrap(), (200, b"body".to_vec()));
        let chunked =
            b"HTTP/1.1 500 X\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nbo\r\n\r\n2\r\ndy\r\n0\r\n\r\n";
        assert_eq!(parse_answer(chunked).unwrap(), (500, b"bo\r\ndy".to_vec()));
        assert!(parse_answer(b"garbage").is_err());
        assert!(
            parse_answer(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err()
        );
    }

    #[test]
    fn the_form_reaches_a_server_and_the_answer_comes_back() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            // read until the closing boundary arrived
            while find(&got, b"--\r\n").is_none() {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            got
        });
        let form = Form {
            fields: &[("language", "ar")],
            file_field: "file",
            file_name: "request.wav",
            file: b"RIFFdata",
        };
        let (status, answer) =
            post_form(&addr, "/inference", &form, Duration::from_secs(5)).unwrap();
        assert_eq!((status, answer.as_slice()), (200, &b"ok"[..]));
        let got = server.join().unwrap();
        let text = String::from_utf8_lossy(&got);
        assert!(text.starts_with("POST /inference HTTP/1.1"));
        assert!(text.contains("name=\"language\"\r\n\r\nar"));
        assert!(text.contains("name=\"file\"; filename=\"request.wav\""));
        assert!(text.contains("RIFFdata"));
    }

    #[test]
    fn a_closed_port_is_not_reachable() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(can_connect(&addr, Duration::from_secs(1)));
        drop(listener);
        assert!(!can_connect(&addr, Duration::from_millis(300)));
    }
}
