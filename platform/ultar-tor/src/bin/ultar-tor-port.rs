//! `ultar-tor-port [DIR] [--unix SOCKET]` – development tool: starts the Tor client of `ultar-tor`
//! with its state in DIR (default: a new temporary folder), prints the HTTP `CONNECT` address
//! (with the secret) and the SOCKS address on standard output, and runs until standard input
//! closes. With `--unix SOCKET` it also opens the Unix socket front (mode 0600, no secret) at
//! that path and prints `unix:SOCKET` as a third line: the way out of an engine in a cell
//! without network. Builders use it to test the browser's engine host through Tor
//! (`scripts/build-servo-engine.sh --shot`).

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut dir = None;
    let mut socket = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--unix" {
            socket = args.next().map(PathBuf::from);
            if socket.is_none() {
                eprintln!("ultar-tor-port: --unix needs a path");
                return ExitCode::from(2);
            }
        } else if dir.is_none() {
            dir = Some(PathBuf::from(arg));
        } else {
            eprintln!("usage: ultar-tor-port [DIR] [--unix SOCKET]");
            return ExitCode::from(2);
        }
    }
    let dir = dir.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("ultar-tor-port-{}", std::process::id()))
    });
    let tor = match ultar_tor::Tor::start(&dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ultar-tor-port: {e}");
            return ExitCode::from(1);
        }
    };
    if let Some(path) = &socket {
        if let Err(e) = tor.unix_proxy(path) {
            eprintln!("ultar-tor-port: {}: {e}", path.display());
            return ExitCode::from(1);
        }
    }
    let mut out = std::io::stdout();
    let mut printed = writeln!(out, "{}\n{}", tor.http_proxy_url(), tor.proxy_url());
    if let (Ok(()), Some(path)) = (&printed, &socket) {
        printed = writeln!(out, "unix:{}", path.display());
    }
    if printed.and_then(|()| out.flush()).is_err() {
        return ExitCode::from(1);
    }
    let mut sink = [0u8; 64];
    while matches!(std::io::stdin().read(&mut sink), Ok(n) if n > 0) {}
    ExitCode::SUCCESS
}
