//! The encrypted data partition as user space sees it (kernel: `kernel/ultar-kern/src/crypt.rs`):
//! its state, the read-only LUKS2 header, handing over the master key, and the console on which
//! `ultar-luks unlock` asks for the passphrase without echo.

use nix::sys::termios::{tcgetattr, tcsetattr, LocalFlags, SetArg};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use zeroize::Zeroizing;

/// The kernel's control file: `none`, `locked` or `open`; takes `open <offset> <hex key>`.
pub const CONTROL: &str = "/sys/kernel/ultar/crypt";
/// The locked data partition, read only (its LUKS2 header).
pub const DATA_NODE: &str = "/dev/ultar/part/ultar-data";
/// The system console.
pub const CONSOLE: &str = "/dev/console";
/// Longest passphrase read from the console.
const MAX_LINE: usize = 1024;

/// Whether the kernel holds an encrypted data partition that waits for its key.
pub fn locked() -> bool {
    std::fs::read_to_string(CONTROL).is_ok_and(|s| s.trim() == "locked")
}

/// The locked data partition for reading its header.
pub fn open_header() -> io::Result<File> {
    File::open(DATA_NODE)
}

/// Hands the master key (lower-case hex) and the data offset to the kernel, which mounts
/// `/data`; fails when the kernel refuses the key. The command is wiped afterwards.
pub fn hand_key(offset: u64, key_hex: &str) -> io::Result<()> {
    let mut command = Zeroizing::new(String::with_capacity(key_hex.len() + 32));
    command.push_str("open ");
    command.push_str(&offset.to_string());
    command.push(' ');
    command.push_str(key_hex);
    OpenOptions::new()
        .write(true)
        .open(CONTROL)?
        .write_all(command.as_bytes())
}

/// The console, for reading and writing.
pub fn console() -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(CONSOLE)
}

/// Reads one line (at most 1 KiB) from `console` with the echo off; the echo comes back
/// afterwards. `None` when the console ends before any input.
pub fn read_hidden(console: &mut File) -> Option<Zeroizing<Vec<u8>>> {
    let old = tcgetattr(&*console).ok();
    if let Some(mut quiet) = old.clone() {
        quiet.local_flags.remove(LocalFlags::ECHO);
        let _ = tcsetattr(&*console, SetArg::TCSANOW, &quiet);
    }
    // room for the longest passphrase: no reallocation leaves copies behind
    let mut line = Zeroizing::new(Vec::with_capacity(MAX_LINE));
    let mut byte = [0u8; 1];
    let mut ended = false;
    while let Ok(1) = console.read(&mut byte) {
        if byte[0] == b'\n' || byte[0] == b'\r' {
            ended = true;
            break;
        }
        if line.len() < MAX_LINE {
            line.push(byte[0]);
        }
    }
    if let Some(old) = old {
        let _ = tcsetattr(&*console, SetArg::TCSANOW, &old);
    }
    let _ = console.write_all(b"\n");
    (ended || !line.is_empty()).then_some(line)
}
