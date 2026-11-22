//! RFS-Server (Remote File System): das Modem (CP) liest und schreibt seine Einstellungen (NV) und Dateien
//! über den Kanal 245 (/dev/umts_rfs0) beim Anwendungsprozessor. Ohne Bediener kann der CP nichts sichern.
//!
//! Rahmen (am A30 mitgeschnitten): Kopf 6 Byte `länge:u32 (gesamt) | befehl:u8 | id:u8`, dann Nutzdaten.
//! Große Nachrichten kommen in Stücken (je read ≤ 2040 Byte) und werden hier anhand der Länge zusammengesetzt.
//!
//! Sicherheit: gearbeitet wird NUR auf einer Kopie (`<dir>/nv_data.bin`, `<dir>/rfs/…`). Die Original-EFS-
//! Partition mit IMEI/Kalibrierung wird nie beschrieben.
//!
//! Befehle: 0x01 NV lesen, 0x02 NV schreiben (Aufbau aus libsamsung-ipc bestätigt);
//! Dateibefehle 0x03–0x14 (Öffnen 0x11 am Gerät gesehen) – Antwortaufbau `ret:i32 | errno:i32 [| daten]`
//! ist eine begründete Annahme und wird im Protokoll mitgeschrieben (// VERIFY).

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

pub const RFS_NV_READ_ITEM: u8 = 0x01;
pub const RFS_NV_WRITE_ITEM: u8 = 0x02;
pub const RFS_READ_FILE: u8 = 0x03;
pub const RFS_WRITE_FILE: u8 = 0x04;
pub const RFS_LSEEK_FILE: u8 = 0x05;
pub const RFS_CLOSE_FILE: u8 = 0x06;
pub const RFS_OPEN_FILE: u8 = 0x11;
pub const RFS_CREATE_FILE: u8 = 0x14;

const ENOENT: i32 = 2;
const EBADF: i32 = 9;
const EIO: i32 = 5;

pub struct Rfs {
    dir: PathBuf,
    /// unvollständige Nachricht
    buf: Vec<u8>,
    files: HashMap<i32, File>,
    next_fd: i32,
    pub log: Vec<String>,
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0)
}

/// Antwortrahmen bauen.
pub fn frame(cmd: u8, id: u8, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(6 + body.len());
    v.extend_from_slice(&((6 + body.len()) as u32).to_le_bytes());
    v.push(cmd);
    v.push(id);
    v.extend_from_slice(body);
    v
}

fn ret_err(ret: i32, err: i32) -> Vec<u8> {
    let mut v = ret.to_le_bytes().to_vec();
    v.extend_from_slice(&err.to_le_bytes());
    v
}

impl Rfs {
    pub fn new(dir: impl Into<PathBuf>) -> Rfs {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(dir.join("rfs"));
        Rfs { dir, buf: Vec::new(), files: HashMap::new(), next_fd: 3, log: Vec::new() }
    }

    fn nv_path(&self) -> PathBuf {
        self.dir.join("nv_data.bin")
    }

    /// Pfad des CP auf den eigenen Ordner abbilden (nur Dateiname, keine Verzeichniswechsel).
    fn map_path(&self, raw: &[u8]) -> Option<PathBuf> {
        let s = String::from_utf8_lossy(raw);
        let name = s.trim_end_matches('\0').rsplit('/').next()?.to_string();
        if name.is_empty() || name == "." || name == ".." || name.contains('\\') {
            return None;
        }
        Some(self.dir.join("rfs").join(name))
    }

    /// Ein gelesenes Stück vom RFS-Kanal einspeisen; liefert die zu sendenden Antworten.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 6 {
                break;
            }
            let len = u32_at(&self.buf, 0) as usize;
            if !(6..=8 << 20).contains(&len) {
                self.log.push(format!("RFS: unplausible Länge {len} – Puffer verworfen"));
                self.buf.clear();
                break;
            }
            if self.buf.len() < len {
                break;
            }
            let msg: Vec<u8> = self.buf.drain(..len).collect();
            out.push(self.handle(msg[4], msg[5], &msg[6..]));
        }
        out
    }

    fn handle(&mut self, cmd: u8, id: u8, d: &[u8]) -> Vec<u8> {
        match cmd {
            RFS_NV_READ_ITEM => {
                let (off, len) = (u32_at(d, 0), u32_at(d, 4));
                let mut data = vec![0xffu8; len.min(4 << 20) as usize];
                let ok = File::open(self.nv_path()).and_then(|mut f| {
                    f.seek(SeekFrom::Start(off as u64))?;
                    f.read_exact(&mut data)
                });
                self.log.push(format!("RFS NV lesen off=0x{off:x} len={len} -> {}", if ok.is_ok() { "ok" } else { "FEHLER" }));
                let mut b = vec![ok.is_ok() as u8];
                b.extend_from_slice(&off.to_le_bytes());
                b.extend_from_slice(&len.to_le_bytes());
                if ok.is_ok() {
                    b.extend_from_slice(&data);
                }
                frame(cmd, id, &b)
            }
            RFS_NV_WRITE_ITEM => {
                let (off, len) = (u32_at(d, 0), u32_at(d, 4));
                let data = d.get(8..8 + len as usize);
                let ok = match data {
                    Some(data) => OpenOptions::new().write(true).create(true).truncate(false).open(self.nv_path()).and_then(|mut f| {
                        f.seek(SeekFrom::Start(off as u64))?;
                        f.write_all(data)?;
                        f.sync_data()
                    }),
                    None => Err(std::io::Error::other("zu kurz")),
                };
                self.log.push(format!("RFS NV schreiben off=0x{off:x} len={len} -> {}", if ok.is_ok() { "ok" } else { "FEHLER" }));
                let mut b = vec![ok.is_ok() as u8];
                b.extend_from_slice(&off.to_le_bytes());
                b.extend_from_slice(&len.to_le_bytes());
                frame(cmd, id, &b)
            }
            RFS_OPEN_FILE | RFS_CREATE_FILE => {
                let (flags, plen) = (u32_at(d, 0), u32_at(d, 4) as usize);
                let raw = d.get(8..(8 + plen).min(d.len())).unwrap_or(&[]);
                let name = String::from_utf8_lossy(raw).trim_end_matches('\0').to_string();
                let res = match self.map_path(raw) {
                    Some(p) => {
                        let acc = flags & 3; // O_RDONLY 0, O_WRONLY 1, O_RDWR 2
                        let create = cmd == RFS_CREATE_FILE || flags & 0o100 != 0;
                        OpenOptions::new().read(acc != 1).write(acc != 0).create(create && acc != 0).truncate(flags & 0o1000 != 0 && acc != 0).open(p)
                    }
                    None => Err(std::io::Error::from_raw_os_error(ENOENT)),
                };
                match res {
                    Ok(f) => {
                        let fd = self.next_fd;
                        self.next_fd += 1;
                        self.files.insert(fd, f);
                        self.log.push(format!("RFS öffnen \"{name}\" flags=0x{flags:x} -> fd {fd}"));
                        frame(cmd, id, &ret_err(fd, 0))
                    }
                    Err(e) => {
                        let en = e.raw_os_error().unwrap_or(ENOENT);
                        self.log.push(format!("RFS öffnen \"{name}\" flags=0x{flags:x} -> Fehler {en}"));
                        frame(cmd, id, &ret_err(-1, en))
                    }
                }
            }
            RFS_READ_FILE => {
                let (fd, len) = (u32_at(d, 0) as i32, (u32_at(d, 4) as usize).min(1 << 20));
                let mut data = vec![0u8; len];
                let r = self.files.get_mut(&fd).ok_or(EBADF).and_then(|f| f.read(&mut data).map_err(|e| e.raw_os_error().unwrap_or(EIO)));
                self.log.push(format!("RFS lesen fd {fd} len={len} -> {r:?}"));
                match r {
                    Ok(n) => {
                        let mut b = ret_err(n as i32, 0);
                        b.extend_from_slice(&data[..n]);
                        frame(cmd, id, &b)
                    }
                    Err(e) => frame(cmd, id, &ret_err(-1, e)),
                }
            }
            RFS_WRITE_FILE => {
                let (fd, len) = (u32_at(d, 0) as i32, u32_at(d, 4) as usize);
                let data = d.get(8..(8 + len).min(d.len())).unwrap_or(&[]);
                let r = self.files.get_mut(&fd).ok_or(EBADF).and_then(|f| f.write(data).map_err(|e| e.raw_os_error().unwrap_or(EIO)));
                self.log.push(format!("RFS schreiben fd {fd} len={len} -> {r:?}"));
                match r {
                    Ok(n) => frame(cmd, id, &ret_err(n as i32, 0)),
                    Err(e) => frame(cmd, id, &ret_err(-1, e)),
                }
            }
            RFS_LSEEK_FILE => {
                let (fd, off, whence) = (u32_at(d, 0) as i32, u32_at(d, 4) as i32, u32_at(d, 8));
                let pos = match whence {
                    1 => SeekFrom::Current(off as i64),
                    2 => SeekFrom::End(off as i64),
                    _ => SeekFrom::Start(off.max(0) as u64),
                };
                let r = self.files.get_mut(&fd).ok_or(EBADF).and_then(|f| f.seek(pos).map_err(|e| e.raw_os_error().unwrap_or(EIO)));
                self.log.push(format!("RFS springen fd {fd} off={off} whence={whence} -> {r:?}"));
                match r {
                    Ok(n) => frame(cmd, id, &ret_err(n as i32, 0)),
                    Err(e) => frame(cmd, id, &ret_err(-1, e)),
                }
            }
            RFS_CLOSE_FILE => {
                let fd = u32_at(d, 0) as i32;
                let ok = self.files.remove(&fd).is_some();
                self.log.push(format!("RFS schließen fd {fd} -> {ok}"));
                frame(cmd, id, &if ok { ret_err(0, 0) } else { ret_err(-1, EBADF) })
            }
            _ => {
                let head: Vec<String> = d.iter().take(48).map(|b| format!("{b:02x}")).collect();
                self.log.push(format!("RFS unbekannter Befehl 0x{cmd:02x} id={id} len={} daten={}", d.len(), head.join(" ")));
                frame(cmd, id, &ret_err(-1, ENOENT))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-rfs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn nv_write_in_chunks_then_read() {
        let d = tmp("nv");
        std::fs::write(d.join("nv_data.bin"), vec![0u8; 4096]).unwrap();
        let mut r = Rfs::new(&d);
        // NV schreiben: off 0x100, 5 Byte – in zwei Stücken geliefert
        let mut body = 0x100u32.to_le_bytes().to_vec();
        body.extend_from_slice(&5u32.to_le_bytes());
        body.extend_from_slice(b"HALLO");
        let msg = frame(RFS_NV_WRITE_ITEM, 7, &body);
        assert!(r.feed(&msg[..9]).is_empty());
        let out = r.feed(&msg[9..]);
        assert_eq!(out.len(), 1);
        assert_eq!(&out[0][..6], &[15, 0, 0, 0, RFS_NV_WRITE_ITEM, 7]);
        assert_eq!(out[0][6], 1); // bestätigt
        // lesen
        let mut body = 0x100u32.to_le_bytes().to_vec();
        body.extend_from_slice(&5u32.to_le_bytes());
        let out = r.feed(&frame(RFS_NV_READ_ITEM, 8, &body));
        assert_eq!(&out[0][15..], b"HALLO");
        assert_eq!(std::fs::read(d.join("nv_data.bin")).unwrap().len(), 4096);
    }

    #[test]
    fn open_missing_and_no_traversal() {
        let d = tmp("open");
        let mut r = Rfs::new(&d);
        let path = b"../../etc/passwd\0";
        let mut body = 0u32.to_le_bytes().to_vec();
        body.extend_from_slice(&(path.len() as u32).to_le_bytes());
        body.extend_from_slice(path);
        let out = r.feed(&frame(RFS_OPEN_FILE, 1, &body));
        // landet als "passwd" im eigenen Ordner -> gibt es nicht
        assert_eq!(i32::from_le_bytes(out[0][6..10].try_into().unwrap()), -1);
        assert_eq!(i32::from_le_bytes(out[0][10..14].try_into().unwrap()), ENOENT);
    }

    #[test]
    fn file_roundtrip() {
        let d = tmp("file");
        let mut r = Rfs::new(&d);
        let path = b"cal.bin\0";
        let mut body = (2u32 | 0o100).to_le_bytes().to_vec();
        body.extend_from_slice(&(path.len() as u32).to_le_bytes());
        body.extend_from_slice(path);
        let out = r.feed(&frame(RFS_OPEN_FILE, 1, &body));
        let fd = i32::from_le_bytes(out[0][6..10].try_into().unwrap());
        assert!(fd >= 3);
        let mut w = (fd as u32).to_le_bytes().to_vec();
        w.extend_from_slice(&3u32.to_le_bytes());
        w.extend_from_slice(b"abc");
        let out = r.feed(&frame(RFS_WRITE_FILE, 2, &w));
        assert_eq!(i32::from_le_bytes(out[0][6..10].try_into().unwrap()), 3);
        assert_eq!(std::fs::read(d.join("rfs/cal.bin")).unwrap(), b"abc");
    }
}
