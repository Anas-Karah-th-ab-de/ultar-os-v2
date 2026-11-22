//! ultar-ctl – Steuerung von ultar-init über /run/ultar/ctl.sock
//! Befehle: status | start <d> | stop <d> | restart <d> | reload | uptime | poweroff | reboot | rescue 1|2|3
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = if args.is_empty() { "status".to_string() } else { args.join(" ") };
    let path = std::env::var("ULTAR_CTL_SOCK").unwrap_or_else(|_| "/run/ultar/ctl.sock".into());
    let mut s = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ultar-ctl: {}", ultar_sprache::tr(&format!("keine Verbindung zu {path}: {e}")));
            std::process::exit(2);
        }
    };
    if s.write_all(format!("{cmd}\n").as_bytes()).is_err() {
        std::process::exit(2);
    }
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    print!("{out}");
    if out.starts_with("Fehler") {
        std::process::exit(1);
    }
}
