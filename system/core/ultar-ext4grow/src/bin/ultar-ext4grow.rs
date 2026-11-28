//! `ultar-ext4grow <image> [bytes]` – grows the unmounted ext4 in an image file to `bytes`
//! (default: the file's size). For tests and image building; the installer uses the library.

use std::process::ExitCode;
use ultar_ext4grow::{grow, Outcome};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: ultar-ext4grow <image> [bytes]");
        return ExitCode::from(2);
    };
    let run = || -> Result<Outcome, String> {
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let size = match args.get(1) {
            Some(b) => b.parse().map_err(|_| "bytes".to_string())?,
            None => f.metadata().map_err(|e| e.to_string())?.len(),
        };
        let r = grow(&mut f, size).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        Ok(r)
    };
    match run() {
        Ok(Outcome::Grown { from, to }) => {
            println!("grown {from} -> {to} blocks");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Unchanged(why)) => {
            println!("unchanged: {why}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ultar-ext4grow: {e}");
            ExitCode::FAILURE
        }
    }
}
