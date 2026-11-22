//! Ton zwischen Geräten: Mithören (Mikrofon eines Geräts hier abspielen) und Durchsage (hier sprechen,
//! dort abspielen) – z. B. Handy spricht durch den Server-Lautsprecher, oder hört eine Platine ohne Bildschirm ab.
//! Format überall: 16 kHz, Mono, 16 Bit (s16le) roh. Geräte: `ultar-shell --mic-roh/--ton-roh`, sonst
//! ALSA-Werkzeuge; frei wählbar über ULTAR_TON_AUFNAHME / ULTAR_TON_WIEDERGABE.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

pub const RATE: u32 = 16_000;

fn befehl(var: &str, shell_arg: &str, alsa: &str) -> Vec<String> {
    if let Ok(v) = std::env::var(var) {
        return v.split_whitespace().map(String::from).collect();
    }
    let shell = std::env::var("ULTAR_SHELL").unwrap_or_else(|_| "ultar-shell".into());
    let hat = |p: &str| std::env::var("PATH").unwrap_or_default().split(':').any(|d| std::path::Path::new(d).join(p).exists()) || std::path::Path::new(p).exists();
    if hat(&shell) && std::path::Path::new("/dev/snd").exists() {
        return vec![shell, shell_arg.into()];
    }
    format!("{alsa} -q -t raw -f S16_LE -r {RATE} -c 1").split_whitespace().map(String::from).collect()
}

/// Aufnahme starten; Blöcke kommen über den Empfänger
pub struct Aufnahme {
    kind: Child,
    pub rx: Receiver<Vec<u8>>,
}

impl Aufnahme {
    pub fn start() -> Result<Aufnahme, String> {
        let b = befehl("ULTAR_TON_AUFNAHME", "--mic-roh", "arecord");
        let mut kind = Command::new(&b[0]).args(&b[1..]).stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).spawn().map_err(|e| format!("Aufnahme ({}): {e}", b[0]))?;
        let mut out = kind.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 3200];
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Aufnahme { kind, rx })
    }

    /// alles bisher Aufgenommene (wartet höchstens `ms` auf den ersten Block)
    pub fn holen(&self, ms: u64) -> Vec<u8> {
        let mut v = Vec::new();
        if let Ok(b) = self.rx.recv_timeout(std::time::Duration::from_millis(ms)) {
            v.extend(b);
        }
        while let Ok(b) = self.rx.try_recv() {
            v.extend(b);
            if v.len() > RATE as usize * 2 * 2 {
                break; // höchstens 2 s je Paket
            }
        }
        v
    }
}

impl Drop for Aufnahme {
    fn drop(&mut self) {
        let _ = self.kind.kill();
        let _ = self.kind.wait();
    }
}

pub struct Wiedergabe {
    kind: Child,
}

impl Wiedergabe {
    pub fn start() -> Result<Wiedergabe, String> {
        let b = befehl("ULTAR_TON_WIEDERGABE", "--ton-roh", "aplay");
        let kind = Command::new(&b[0]).args(&b[1..]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("Wiedergabe ({}): {e}", b[0]))?;
        Ok(Wiedergabe { kind })
    }

    pub fn spielen(&mut self, pcm: &[u8]) -> Result<(), String> {
        self.kind.stdin.as_mut().ok_or("geschlossen")?.write_all(pcm).map_err(|e| e.to_string())
    }
}

impl Drop for Wiedergabe {
    fn drop(&mut self) {
        drop(self.kind.stdin.take());
        let _ = self.kind.wait();
    }
}
