//! `ultar-tor-port [DIR]` – development tool: starts the Tor client of `ultar-tor` with its state
//! in DIR (default: a new temporary folder), prints the HTTP `CONNECT` address (with the secret)
//! and the SOCKS address on standard output, and runs until standard input closes. Builders use
//! it to test the browser's engine host through Tor (`scripts/build-servo-engine.sh --shot`).

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("ultar-tor-port-{}", std::process::id()))
        });
    let tor = match ultar_tor::Tor::start(&dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ultar-tor-port: {e}");
            return ExitCode::from(1);
        }
    };
    let mut out = std::io::stdout();
    let printed =
        writeln!(out, "{}\n{}", tor.http_proxy_url(), tor.proxy_url()).and_then(|()| out.flush());
    if printed.is_err() {
        return ExitCode::from(1);
    }
    let mut sink = [0u8; 64];
    while matches!(std::io::stdin().read(&mut sink), Ok(n) if n > 0) {}
    ExitCode::SUCCESS
}
