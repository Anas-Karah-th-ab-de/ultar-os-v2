//! `ultar-luks` – the LUKS2 tool of Ultar OS.
//!
//! - `unlock`: start of the installed system (ultar-init runs it before the services). When the
//!   kernel reports the data partition as locked (`/sys/kernel/ultar/crypt` = `locked`), asks
//!   for the passphrase on the console (no echo), opens the header read from
//!   `/dev/ultar/part/ultar-data` and hands the master key to the kernel (`open <offset> <hex>`);
//!   the kernel then mounts `/data`. Asks again after a wrong passphrase.
//! - `format <image> <plain> [--test]`, `check <image>`, `decrypt <image> <out> <bytes>`:
//!   image files for the interoperability test with cryptsetup (`tools/kern-test/luks-interop.sh`).
//!   The passphrase is read from stdin (one line).

use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::FileExt;
use std::process::ExitCode;
use ultar_hal_system::crypt;
use ultar_luks::{Error, Params, Sectors, DATA_OFFSET, SECTOR};
use zeroize::Zeroizing;

const CHUNK: usize = 1 << 20;

fn stdin_line() -> Zeroizing<String> {
    let mut s = Zeroizing::new(String::new());
    let _ = std::io::stdin().lock().read_line(&mut s);
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    s
}

fn reader(f: &File) -> impl FnMut(u64, &mut [u8]) -> Result<(), Error> + '_ {
    move |at, buf| f.read_exact_at(buf, at).map_err(Error::from)
}

fn text<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn format(image: &str, plain: &str, params: &Params) -> Result<(), String> {
    let pass = stdin_line();
    // with the start token, as the installer writes it (cryptsetup must keep accepting it)
    let start = ultar_luks::StartInfo {
        lang: "en".into(),
        keyboard: "us".into(),
    };
    let f = ultar_luks::format_with(pass.as_bytes(), params, Some(&start)).map_err(text)?;
    let out = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(image)
        .map_err(text)?;
    let mut meta = f.metadata.clone();
    meta.resize(DATA_OFFSET as usize, 0);
    out.write_all_at(&meta, 0).map_err(text)?;
    let mut src = File::open(plain).map_err(text)?;
    let s = Sectors::from_key(&f.key);
    let mut buf = vec![0u8; CHUNK];
    let mut off = 0u64;
    loop {
        let got = src.read(&mut buf).map_err(text)?;
        if got == 0 {
            break;
        }
        let n = got.div_ceil(SECTOR as usize) * SECTOR as usize;
        buf[got..n].fill(0);
        s.encrypt(off / SECTOR, &mut buf[..n]);
        out.write_all_at(&buf[..n], DATA_OFFSET + off)
            .map_err(text)?;
        off += n as u64;
    }
    out.sync_all().map_err(text)
}

fn check(image: &str) -> Result<(), String> {
    let pass = stdin_line();
    let f = File::open(image).map_err(text)?;
    let o = ultar_luks::open(&mut reader(&f), pass.as_bytes()).map_err(text)?;
    println!("ok offset={}", o.offset);
    Ok(())
}

fn decrypt(image: &str, out: &str, bytes: u64) -> Result<(), String> {
    let pass = stdin_line();
    let f = File::open(image).map_err(text)?;
    let o = ultar_luks::open(&mut reader(&f), pass.as_bytes()).map_err(text)?;
    let s = Sectors::from_key(&o.key);
    let mut dst = File::create(out).map_err(text)?;
    let mut buf = vec![0u8; CHUNK];
    let mut off = 0u64;
    while off < bytes {
        let n = ((bytes - off).min(CHUNK as u64).div_ceil(SECTOR) * SECTOR) as usize;
        f.read_exact_at(&mut buf[..n], o.offset + off)
            .map_err(text)?;
        s.decrypt(off / SECTOR, &mut buf[..n]);
        let keep = ((bytes - off) as usize).min(n);
        dst.write_all(&buf[..keep]).map_err(text)?;
        off += n as u64;
    }
    Ok(())
}

fn hex(key: &[u8]) -> Zeroizing<String> {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = Zeroizing::new(String::with_capacity(key.len() * 2));
    for b in key {
        s.push(char::from(D[usize::from(b >> 4)]));
        s.push(char::from(D[usize::from(b & 15)]));
    }
    s
}

fn unlock() -> Result<(), String> {
    if !crypt::locked() {
        return Ok(());
    }
    let data = crypt::open_header().map_err(|e| format!("{}: {e}", crypt::DATA_NODE))?;
    let mut console = crypt::console().map_err(|e| format!("{}: {e}", crypt::CONSOLE))?;
    let title = format!(
        "\n{}: Daten verschluesselt / data encrypted\n",
        ultar_brand::PRODUCT
    );
    let _ = console.write_all(title.as_bytes());
    loop {
        let Some(pass) = crypt::read_hidden(&mut console, b"Passwort / passphrase: ") else {
            return Err("no console input".into());
        };
        match ultar_luks::open(&mut reader(&data), &pass) {
            Ok(o) => {
                crypt::hand_key(o.offset, &hex(&o.key[..]))
                    .map_err(|e| format!("kernel refused the key: {e}"))?;
                let _ = console.write_all(b"Daten entsperrt / data unlocked\n");
                return Ok(());
            }
            Err(Error::Passphrase) => {
                let _ = console.write_all(b"Falsches Passwort / wrong passphrase\n");
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let params = if args.iter().any(|a| a == "--test") {
        Params::TEST
    } else {
        Params::INSTALL
    };
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--test")
        .collect();
    let r = match words.as_slice() {
        ["unlock"] => unlock(),
        ["format", image, plain] => format(image, plain, &params),
        ["check", image] => check(image),
        ["decrypt", image, out, bytes] => bytes.parse().map_err(|_| "bytes".to_string()).and_then(|b| decrypt(image, out, b)),
        _ => Err("usage: ultar-luks unlock | format <image> <plain> [--test] | check <image> | decrypt <image> <out> <bytes>".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ultar-luks: {e}");
            ExitCode::FAILURE
        }
    }
}
