//! Dateisysteme des Ultar-Kerns.
//!
//! - FAT32/FAT16 lesen (lange Dateinamen) – Startpartition `ULTAR-EFI` oder ein ganzer FAT-Datenträger.
//! - Arbeitsspeicher-Schicht: neue/geänderte Dateien landen im RAM (nicht dauerhaft) und überdecken FAT.
//! - Geräte: /dev/null, /dev/zero, /dev/tty, /dev/console, /dev/urandom.
//! Ein Dateibaum: Pfade werden zuerst im RAM, dann auf FAT gesucht.

use crate::driver::BLOCK;
use crate::isolation::tmpfs;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone)]
pub struct DirEnt {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

pub enum Node {
    File(Arc<Vec<u8>>),
    Dir(Vec<DirEnt>),
    Dev(&'static str),
}

struct Fat {
    disk: usize,
    part: u64,
    bps: u64,
    spc: u64,
    fat_start: u64,
    data_start: u64,
    root_clus: u32,
    fat32: bool,
    root_start: u64,
    root_secs: u64,
}

enum Mnt {
    Fat(Fat),
    /// ext4; beschreibbar nur bei Ultar-eigenen Partitionen (ultar-compat, ultar-data)
    Ext4(crate::ext4::Ext4, bool),
}

/// Einhängepunkte: (Pfad, Dateisystem) – längster passender Pfad gewinnt
static MOUNTS: Mutex<Vec<(String, Mnt)>> = Mutex::new(Vec::new());
/// Programme, die `busybox` mitbringt (für /bin/<name> ohne eigene Datei)
static APPLETS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// RAM-Schicht: Pfad → Inhalt; Verzeichnisse als Schlüssel mit abschließendem '/'
pub static RAM: Mutex<BTreeMap<String, Arc<Vec<u8>>>> = Mutex::new(BTreeMap::new());

static FATCACHE: Mutex<(u64, Vec<u8>, usize)> = Mutex::new((0, Vec::new(), 0));

const DEVS: [&str; 6] = ["null", "zero", "tty", "console", "urandom", "random"];
/// Geräte für Oberflächenprogramme (nur mit Bildschirm)
const UI_DEVS: [&str; 3] = ["fb0", "input/event0", "input/event1"];

fn rd(disk: usize, lba: u64, n: usize) -> Option<Vec<u8>> {
    // up to 4 MiB at once: refused memory reads as a failed read
    let mut b = crate::mem::try_vec(n * 512)?;
    let mut bl = BLOCK.lock();
    bl.get_mut(disk)?.read(lba, &mut b).ok()?;
    Some(b)
}

fn le16(b: &[u8], o: usize) -> u64 {
    u16::from_le_bytes([b[o], b[o + 1]]) as u64
}
fn le32(b: &[u8], o: usize) -> u64 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as u64
}

fn try_fat(disk: usize, part: u64) -> Option<Fat> {
    let b = rd(disk, part, 1)?;
    if b[510] != 0x55 || b[511] != 0xAA {
        return None;
    }
    let bps = le16(&b, 11);
    let spc = b[13] as u64;
    if bps != 512 || spc == 0 || !spc.is_power_of_two() {
        return None;
    }
    let reserved = le16(&b, 14);
    let nfats = b[16] as u64;
    let root_ent = le16(&b, 17);
    let total = if le16(&b, 19) != 0 { le16(&b, 19) } else { le32(&b, 32) };
    let fatsz = if le16(&b, 22) != 0 { le16(&b, 22) } else { le32(&b, 36) };
    if nfats == 0 || fatsz == 0 {
        return None;
    }
    let root_secs = (root_ent * 32).div_ceil(bps);
    let fat_start = part + reserved;
    let root_start = fat_start + nfats * fatsz;
    let data_start = root_start + root_secs;
    let clusters = (total - (data_start - part)) / spc;
    if clusters < 4085 {
        return None; // FAT12 nicht unterstützt
    }
    let fat32 = clusters >= 65525;
    Some(Fat { disk, part, bps, spc, fat_start, data_start, root_clus: le32(&b, 44) as u32, fat32, root_start, root_secs })
}

/// The running slot's system partition (on the boot disk), taken out of `exts`. Missing: the
/// start failed (a trial restarts, see bootslot.rs).
fn take_slot_root(exts: &mut Vec<(String, crate::ext4::Ext4, String)>, slot: ultar_slots::Slot) -> Option<(crate::ext4::Ext4, String)> {
    let name = ultar_slots::layout::system_part(slot);
    let at = crate::rawpart::locate(name).map(|(disk, first)| alloc::format!("{}:{first}", BLOCK.lock()[disk].name()));
    let i = exts.iter().position(|e| e.0 == name && Some(&e.2) == at.as_ref());
    match i {
        Some(i) => {
            let (_, x, w) = exts.remove(i);
            Some((x, w))
        }
        None => {
            crate::bootslot::start_failed(name);
            None
        }
    }
}

/// Dateisysteme suchen und einhängen: ext4 `ultar-compat` (Ubuntu) als /, `ultar-data` als /data,
/// FAT-Startpartition als /boot/efi – ohne ext4 wird FAT selbst zu /.
pub fn mount() {
    let n = BLOCK.lock().len();
    let mut exts: Vec<(String, crate::ext4::Ext4, String)> = Vec::new();
    let mut installer_platten: Vec<String> = Vec::new();
    let mut fats: Vec<(Fat, String)> = Vec::new();
    for disk in 0..n {
        let dname = BLOCK.lock()[disk].name();
        let mut cands: Vec<(u64, String)> = vec![(0, String::new())];
        if let Some(h) = rd(disk, 1, 1) {
            if &h[0..8] == b"EFI PART" {
                let lba = le32(&h, 72) | le32(&h, 76) << 32;
                let cnt = le32(&h, 80).min(128) as usize;
                let esz = le32(&h, 84) as usize;
                if let Some(t) = rd(disk, lba, (cnt * esz).div_ceil(512)) {
                    for i in 0..cnt {
                        let e = &t[i * esz..(i + 1) * esz];
                        if e[..16].iter().all(|&x| x == 0) {
                            continue;
                        }
                        let name: String = char::decode_utf16(e[56..128].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0)).map(|r| r.unwrap_or('?')).collect();
                        if name == "ULTAR-INSTALL" {
                            installer_platten.push(alloc::format!("{dname}:"));
                        }
                        cands.push((le32(e, 32) | le32(e, 36) << 32, name));
                    }
                }
            }
        }
        for (p, name) in cands {
            if let Some(x) = crate::ext4::Ext4::probe(disk, p) {
                exts.push((name, x, alloc::format!("{dname}:{p}")));
            } else if let Some(f) = try_fat(disk, p) {
                fats.push((f, alloc::format!("{dname}:{p}")));
            }
        }
    }
    // A/B start: only the running slot's system (read only) and the data partition of the boot disk;
    // the slots' partitions are never mounted otherwise (raw access only, rawpart.rs)
    let slot_root = crate::bootslot::slot().map(|s| take_slot_root(&mut exts, s));
    exts.retain(|e| !crate::rawpart::is_ab_name(&e.0));
    if let Some(disk) = crate::rawpart::boot_disk() {
        let own = alloc::format!("{}:", BLOCK.lock()[disk].name());
        exts.retain(|e| e.0 != "ultar-data" || e.2.starts_with(own.as_str()));
    }
    let mut m = MOUNTS.lock();
    if crate::bootslot::slot().is_some() || crate::bootslot::refused() {
        exts.retain(|e| e.0 != "ultar-compat");
    }
    if let Some(Some((x, w))) = slot_root {
        crate::kprintln!("[fs] ext4 on {w} as / (slot system, read only)");
        m.push((String::from("/"), Mnt::Ext4(x, false)));
    }
    // ext4: ultar-compat (sonst erstes ext4 außer ultar-data) als /, ultar-data als /data, weitere unter /media
    let vom_installer = crate::efi::VOM_INSTALLER.load(core::sync::atomic::Ordering::Relaxed);
    let auf_installer = |w: &str| installer_platten.iter().any(|d| w.starts_with(d.as_str()));
    let legacy = crate::bootslot::slot().is_none() && !crate::bootslot::refused();
    let root_idx = exts
        .iter()
        .position(|e| e.0 == "ultar-compat" && auf_installer(&e.2) == vom_installer)
        .or_else(|| exts.iter().position(|e| e.0 == "ultar-compat"))
        .or_else(|| exts.iter().position(|e| e.0 != "ultar-data"))
        .filter(|_| legacy);
    let mut root_taken = !m.is_empty();
    let mut root_platte = crate::rawpart::boot_disk().map(|d| alloc::format!("{}:", BLOCK.lock()[d].name())).unwrap_or_default();
    if let Some(i) = root_idx {
        let (name, x, w) = exts.remove(i);
        root_platte = alloc::format!("{}:", w.split(':').next().unwrap_or(""));
        let rw = name.starts_with("ultar-");
        crate::kprintln!("[fs] ext4 auf {w} als /{}", if rw { "" } else { " (nur lesen)" });
        m.push((String::from("/"), Mnt::Ext4(x, rw)));
        root_taken = true;
    }
    if let Some(i) = exts.iter().position(|e| e.0 == "ultar-data") {
        let (_, x, w) = exts.remove(i);
        crate::kprintln!("[fs] ext4 auf {w} als /data");
        m.push((String::from("/data"), Mnt::Ext4(x, true)));
    }
    for (name, x, w) in exts {
        let at = alloc::format!("/media/{}", w.replace(':', "-"));
        crate::kprintln!("[fs] ext4 auf {w} als {at} (nur lesen)");
        let _ = name;
        m.push((at, Mnt::Ext4(x, false)));
    }
    // FAT: die Partition mit dem Ultar-Kern (Ordner ultar-kern oder bin/kerntest) wird /boot/efi bzw. /
    let hat_kern = |f: &Fat| f.lookup("ultar-kern").is_some() || f.lookup("bin/kerntest").is_some();
    // Startpartition derselben Platte wie die Wurzel bevorzugen (installiertes System neben dem Installationsmedium)
    // A/B start: the start partition of the boot disk (it holds only the loader)
    let ab_esp = crate::rawpart::boot_disk().and_then(|_| fats.iter().position(|(_, w)| w.starts_with(root_platte.as_str())));
    let main = ab_esp
        .or_else(|| fats.iter().position(|(f, w)| !root_platte.is_empty() && w.starts_with(root_platte.as_str()) && hat_kern(f)))
        .or_else(|| fats.iter().position(|(f, _)| hat_kern(f)))
        .unwrap_or(0);
    for (i, (f, w)) in fats.into_iter().enumerate() {
        let at = if i == main { String::from(if root_taken { "/boot/efi" } else { "/" }) } else { alloc::format!("/media/{}", w.replace(':', "-")) };
        crate::kprintln!("[fs] FAT{} auf {w} als {at}", if f.fat32 { 32 } else { 16 });
        m.push((at, Mnt::Fat(f)));
    }
    if m.is_empty() {
        crate::kprintln!("[fs] kein Dateisystem gefunden – nur RAM-Dateien");
    }
    drop(m);
    // busybox-Programme als /bin/<name> verfügbar machen
    for list in ["/bin/.applets", "/boot/efi/bin/.applets"] {
        if let Some(Node::File(d)) = open(list) {
            *APPLETS.lock() = String::from_utf8_lossy(&d).lines().map(String::from).collect();
            break;
        }
    }
}

impl Fat {
    fn next(&self, c: u32) -> u32 {
        let (off, sz) = if self.fat32 { (c as u64 * 4, 4) } else { (c as u64 * 2, 2) };
        let sec = self.fat_start + off / self.bps;
        let b = {
            let mut c = FATCACHE.lock();
            if c.0 != sec || c.2 != self.disk || c.1.is_empty() {
                // the cached sector outlives the reader: it belongs to the kernel
                let Some(b) = crate::heap::kernel(|| rd(self.disk, sec, 8)) else { return 0x0fff_ffff };
                *c = (sec, b, self.disk);
            }
            c.1[..512].to_vec()
        };
        let o = (off % self.bps) as usize;
        if sz == 4 {
            (le32(&b, o) & 0x0fff_ffff) as u32
        } else {
            let v = le16(&b, o) as u32;
            if v >= 0xfff8 {
                0x0fff_ffff
            } else {
                v
            }
        }
    }
    fn chain(&self, mut c: u32, limit: u64) -> Vec<u8> {
        let csize = self.spc * self.bps;
        let mut clus = Vec::new();
        while (2..0x0fff_fff7).contains(&c) && (clus.len() as u64) * csize < limit.min(1 << 30) {
            if !crate::mem::try_reserve(&mut clus, 1) {
                return Vec::new();
            }
            clus.push(c);
            c = self.next(c);
        }
        // the whole file is read into memory: without memory for it the file reads as empty
        let Some(mut out) = crate::mem::try_with_capacity(clus.len() * csize as usize) else { return Vec::new() };
        let mut i = 0;
        while i < clus.len() {
            // zusammenhängende Cluster in einem Zug lesen (höchstens 4 MB)
            let mut j = i + 1;
            while j < clus.len() && clus[j] == clus[j - 1] + 1 && ((j - i) as u64) * csize < (4 << 20) {
                j += 1;
            }
            let lba = self.data_start + (clus[i] as u64 - 2) * self.spc;
            match rd(self.disk, lba, (j - i) * self.spc as usize) {
                Some(b) => out.extend_from_slice(&b),
                None => break,
            }
            i = j;
        }
        out.truncate(limit.min(out.len() as u64) as usize);
        out
    }
    fn dir_raw(&self, clus: u32) -> Vec<u8> {
        if clus == 0 && !self.fat32 {
            return rd(self.disk, self.root_start, self.root_secs as usize).unwrap_or_default();
        }
        let c = if clus == 0 { self.root_clus } else { clus };
        self.chain(c, u64::MAX)
    }
    /// Einträge: (Name, Verzeichnis?, Größe, Startcluster)
    fn list(&self, clus: u32) -> Vec<(String, bool, u64, u32)> {
        let raw = self.dir_raw(clus);
        let mut out = Vec::new();
        let mut lfn: Vec<u16> = Vec::new();
        for e in raw.chunks_exact(32) {
            if e[0] == 0 {
                break;
            }
            if e[0] == 0xE5 {
                lfn.clear();
                continue;
            }
            if e[11] == 0x0F {
                let seq = (e[0] & 0x1f) as usize;
                let mut part = Vec::new();
                for &(a, n) in &[(1usize, 5usize), (14, 6), (28, 2)] {
                    for k in 0..n {
                        part.push(u16::from_le_bytes([e[a + 2 * k], e[a + 2 * k + 1]]));
                    }
                }
                if e[0] & 0x40 != 0 {
                    lfn = vec![0xffff; seq * 13];
                }
                if seq >= 1 && seq * 13 <= lfn.len() {
                    lfn[(seq - 1) * 13..seq * 13].copy_from_slice(&part);
                }
                continue;
            }
            if e[11] & 0x08 != 0 {
                lfn.clear();
                continue; // Datenträgername
            }
            let name = if !lfn.is_empty() {
                let v: Vec<u16> = lfn.iter().copied().take_while(|&u| u != 0 && u != 0xffff).collect();
                String::from_utf16_lossy(&v)
            } else {
                let base = core::str::from_utf8(&e[0..8]).unwrap_or("").trim_end();
                let ext = core::str::from_utf8(&e[8..11]).unwrap_or("").trim_end();
                let (lb, le) = (e[12] & 0x08 != 0, e[12] & 0x10 != 0);
                let base = if lb { base.to_lowercase() } else { base.to_string() };
                let ext = if le { ext.to_lowercase() } else { ext.to_string() };
                if ext.is_empty() {
                    base
                } else {
                    alloc::format!("{base}.{ext}")
                }
            };
            lfn.clear();
            if name == "." || name == ".." {
                continue;
            }
            let c = (le16(e, 26) | le16(e, 20) << 16) as u32;
            out.push((name, e[11] & 0x10 != 0, le32(e, 28), c));
        }
        out
    }
    /// vorhandene Datei an Ort und Stelle überschreiben (Größe und Cluster bleiben, Rest mit Leerzeichen)
    fn overwrite(&self, path: &str, data: &[u8]) -> bool {
        let Some((false, size, mut c)) = self.lookup(path) else { return false };
        let csize = (self.spc * self.bps) as usize;
        let mut off = 0usize;
        while (2..0x0fff_fff7).contains(&c) && off < size as usize {
            let mut blk = alloc::vec![b' '; csize];
            let end = (off + csize).min(data.len());
            if off < end {
                blk[..end - off].copy_from_slice(&data[off..end]);
            }
            let lba = self.data_start + (c as u64 - 2) * self.spc;
            let ok = crate::driver::BLOCK.lock().get_mut(self.disk).map(|d| d.write(lba, &blk).is_ok()).unwrap_or(false);
            if !ok {
                return false;
            }
            off += csize;
            c = self.next(c);
        }
        true
    }
    fn lookup(&self, path: &str) -> Option<(bool, u64, u32)> {
        let mut cur = (true, 0u64, 0u32);
        for comp in path.split('/').filter(|s| !s.is_empty()) {
            if !cur.0 {
                return None;
            }
            let ents = self.list(cur.2);
            let e = ents.into_iter().find(|e| e.0.eq_ignore_ascii_case(comp))?;
            cur = (e.1, e.2, e.3);
        }
        Some(cur)
    }
}

/// Pfad normalisieren (absolut, ohne . und ..)
pub fn normalize(cwd: &str, p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let full = if p.starts_with('/') { String::from(p) } else { alloc::format!("{cwd}/{p}") };
    for c in full.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut s = String::from("/");
    s.push_str(&parts.join("/"));
    s
}

/// Whether `path` is a directory of the RAM layer (created there, or the parent of an entry) – the
/// same answer as `ram_dir(path).is_some()` without building the listing.
pub(crate) fn ram_is_dir(path: &str) -> bool {
    let pre = if path == "/" { String::from("/") } else { alloc::format!("{path}/") };
    let ram = RAM.lock();
    let found = ram.range(pre.clone()..).next().is_some_and(|(k, _)| k.starts_with(&pre));
    found
}

pub(crate) fn ram_dir(path: &str) -> Option<Vec<DirEnt>> {
    let ram = RAM.lock();
    let pre = if path == "/" { String::from("/") } else { alloc::format!("{path}/") };
    let mut out: BTreeMap<String, DirEnt> = BTreeMap::new();
    let mut found = ram.contains_key(&pre);
    for (k, v) in ram.range(pre.clone()..) {
        if !k.starts_with(&pre) {
            break;
        }
        found = true;
        let rest = &k[pre.len()..];
        // private trees of namespaces (isolation::tmpfs) are not part of the visible tree
        if rest.is_empty() || rest.starts_with('\0') {
            continue;
        }
        let (name, dir) = match rest.find('/') {
            Some(i) => (&rest[..i], true),
            None => (rest, false),
        };
        out.insert(name.to_string(), DirEnt { name: name.to_string(), dir, size: if dir { 0 } else { v.len() as u64 } });
    }
    if !found {
        return None;
    }
    // the listing of a large directory is one large buffer: it may be refused
    let mut list = crate::mem::try_with_capacity(out.len())?;
    list.extend(out.into_values());
    Some(list)
}

fn mnt_open(m: &Mnt, rel: &str) -> Option<Node> {
    match m {
        Mnt::Fat(f) => match f.lookup(rel)? {
            (false, size, c) => Some(Node::File(Arc::new(f.chain(c, size)))),
            (true, _, c) => Some(Node::Dir(f.list(c).into_iter().map(|e| DirEnt { name: e.0, dir: e.1, size: e.2 }).collect())),
        },
        Mnt::Ext4(x, _) => {
            let (_, ino) = x.lookup(rel)?;
            if ino.is_dir() {
                Some(Node::Dir(
                    x.list(&ino)
                        .into_iter()
                        .map(|(name, _, t)| DirEnt { name, dir: t == 2, size: 0 })
                        .collect(),
                ))
            } else {
                Some(Node::File(Arc::new(x.read(&ino, 1 << 31))))
            }
        }
    }
}

fn fs_open(path: &str) -> Option<Node> {
    let m = MOUNTS.lock();
    let best = m.iter().filter(|(p, _)| p == "/" || path == p || path.starts_with(&alloc::format!("{p}/"))).max_by_key(|(p, _)| p.len())?;
    let rel = &path[best.0.len().min(path.len())..];
    mnt_open(&best.1, rel)
}

/// `/dev/ultar`: `part` and the raw A/B partition nodes (rawpart.rs), the service nodes
/// `hciN` and `uinput` (svcdev)
fn raw_node(d: &str) -> Option<Node> {
    if let Some(n) = crate::svcdev::node(d) {
        return Some(Node::Dev(n));
    }
    let names = crate::rawpart::names();
    match d {
        "ultar" => {
            let mut v: Vec<DirEnt> = crate::svcdev::names().into_iter().map(|name| DirEnt { name, dir: false, size: 0 }).collect();
            if !names.is_empty() {
                v.push(DirEnt { name: "part".into(), dir: true, size: 0 });
            }
            Some(Node::Dir(v))
        }
        _ if names.is_empty() => None,
        "ultar/part" => Some(Node::Dir(names.into_iter().map(|name| DirEnt { name, dir: false, size: 0 }).collect())),
        _ => crate::rawpart::node(d).map(Node::Dev),
    }
}

/// Knoten unter absolutem Pfad
pub fn open(path: &str) -> Option<Node> {
    if tmpfs::is_private(path) {
        return tmpfs::open(path);
    }
    if let Some(d) = path.strip_prefix("/dev/") {
        if let Some(n) = DEVS.iter().find(|&&x| x == d) {
            return Some(Node::Dev(n));
        }
        if let Some(n) = raw_node(d) {
            return Some(n);
        }
        if crate::console::framebuffer().is_some() {
            if let Some(n) = UI_DEVS.iter().find(|&&x| x == d) {
                return Some(Node::Dev(n));
            }
        }
        // input nodes: the fixed ones with a screen, USB keyboards while attached
        if d.strip_prefix("input/").and_then(crate::input::parse).is_some_and(|n| n >= crate::input::FIRST && crate::input::exists(n)) {
            return Some(Node::Dev("input/usb"));
        }
        if d == "input" && !crate::input::all().is_empty() {
            return Some(Node::Dir(crate::input::all().into_iter().map(|n| DirEnt { name: alloc::format!("event{n}"), dir: false, size: 0 }).collect()));
        }
    }
    if path == "/dev" {
        let mut v: Vec<DirEnt> = DEVS.iter().map(|n| DirEnt { name: n.to_string(), dir: false, size: 0 }).collect();
        if crate::console::framebuffer().is_some() {
            v.push(DirEnt { name: "fb0".into(), dir: false, size: 0 });
        }
        if !crate::input::all().is_empty() {
            v.push(DirEnt { name: "input".into(), dir: true, size: 0 });
        }
        // the service nodes (svcdev) are always there
        v.push(DirEnt { name: "ultar".into(), dir: true, size: 0 });
        return Some(Node::Dir(v));
    }
    if path == "/proc" || path.starts_with("/proc/") {
        return crate::procfs::open(path);
    }
    if crate::devices::sysfs::is_tree(path) {
        return crate::devices::sysfs::open(path);
    }
    if crate::groups::files::is_tree(path) {
        return crate::groups::files::open(path);
    }
    if let Some(v) = RAM.lock().get(path) {
        // the stored buffer is shared with the descriptor until it is written (copy on write)
        return Some(Node::File(v.clone()));
    }
    let ramd = ram_dir(path);
    match (fs_open(path), ramd) {
        (Some(Node::Dir(mut v)), r) => {
            // Einhängepunkte und /dev, /proc in der Liste zeigen
            let mut extra: Vec<String> = MOUNTS.lock().iter().filter_map(|(p, _)| {
                let parent = normalize(p, "..");
                (p != "/" && parent == path).then(|| String::from(p.rsplit('/').next().unwrap_or("")))
            }).collect();
            if path == "/" {
                extra.push("dev".into());
                extra.push("proc".into());
                extra.push("sys".into());
                if MOUNTS.lock().iter().any(|(p, _)| p.starts_with("/media/")) {
                    extra.push("media".into());
                }
            }
            for e in extra {
                if !v.iter().any(|x| x.name == e) {
                    v.push(DirEnt { name: e, dir: true, size: 0 });
                }
            }
            for e in r.unwrap_or_default() {
                if !v.iter().any(|x| x.name == e.name) {
                    v.push(e);
                }
            }
            Some(Node::Dir(v))
        }
        (Some(n), _) => Some(n),
        (None, Some(r)) => Some(Node::Dir(r)),
        (None, None) => {
            if path == "/" {
                return Some(Node::Dir(vec![DirEnt { name: "dev".into(), dir: true, size: 0 }, DirEnt { name: "proc".into(), dir: true, size: 0 }, DirEnt { name: "sys".into(), dir: true, size: 0 }]));
            }
            // Zwischenverzeichnis eines Einhängepunkts (z. B. /boot für /boot/efi)
            if MOUNTS.lock().iter().any(|(p, _)| p.starts_with(&alloc::format!("{path}/"))) {
                return Some(Node::Dir(Vec::new()));
            }
            // busybox-Programm unter /bin/<name>
            for d in ["/bin/", "/usr/bin/", "/sbin/", "/usr/sbin/"] {
                if let Some(name) = path.strip_prefix(d) {
                    if !name.contains('/') && APPLETS.lock().iter().any(|a| a == name) {
                        for b in ["/bin/busybox", "/boot/efi/bin/busybox"] {
                            if let Some(Node::File(f)) = open(b) {
                                return Some(Node::File(f));
                            }
                        }
                    }
                }
            }
            None
        }
    }
}

/// Art und Größe ohne den Inhalt zu lesen: (0 Datei, 1 Verzeichnis, 2 Gerät), Größe
pub fn stat(path: &str) -> Option<(u8, u64)> {
    if tmpfs::is_private(path) {
        return tmpfs::stat(path);
    }
    if let Some(v) = RAM.lock().get(path) {
        return Some((0, v.len() as u64));
    }
    if !path.starts_with("/dev") && !path.starts_with("/proc") {
        let m = MOUNTS.lock();
        if let Some(best) = m.iter().filter(|(p, _)| p == "/" || path == p || path.starts_with(&alloc::format!("{p}/"))).max_by_key(|(p, _)| p.len()) {
            let rel = &path[best.0.len().min(path.len())..];
            let r = match &best.1 {
                Mnt::Fat(f) => f.lookup(rel).map(|(d, s, _)| (d as u8, s)),
                Mnt::Ext4(x, _) => x.lookup(rel).map(|(_, i)| (i.is_dir() as u8, i.size)),
            };
            if r.is_some() {
                return r;
            }
        }
    }
    // a directory of the RAM layer: known without building its listing
    if !path.starts_with("/dev") && !path.starts_with("/proc") && !crate::groups::files::is_tree(path) && ram_is_dir(path) {
        return Some((1, 0));
    }
    match open(path)? {
        Node::File(d) => Some((0, d.len() as u64)),
        Node::Dir(_) => Some((1, 0)),
        Node::Dev(_) => Some((2, 0)),
    }
}

/// ext4-Einhängepunkt für einen Pfad, wenn dort geschrieben werden kann: (Mount-Index, relativer Pfad)
fn ext4_target(path: &str) -> Option<(usize, String)> {
    if tmpfs::is_private(path) {
        return None;
    }
    if ["/dev", "/proc", "/tmp", "/run", "/sys"].iter().any(|d| path == *d || path.starts_with(&alloc::format!("{d}/"))) {
        return None;
    }
    let m = MOUNTS.lock();
    let (i, (p, mnt)) = m.iter().enumerate().filter(|(_, (p, _))| p == "/" || path == p || path.starts_with(&alloc::format!("{p}/"))).max_by_key(|(_, (p, _))| p.len())?;
    match mnt {
        Mnt::Ext4(x, true) if x.writable() => Some((i, String::from(&path[p.len().min(path.len())..]))),
        _ => None,
    }
}

fn with_ext4<R>(i: usize, f: impl FnOnce(&crate::ext4::Ext4) -> R) -> Option<R> {
    let m = MOUNTS.lock();
    match &m.get(i)?.1 {
        Mnt::Ext4(x, _) => Some(f(x)),
        _ => None,
    }
}

/// Whether the RAM layer holds a file or a directory entry at `path` (it covers what is on disk).
pub fn ram_has(path: &str) -> bool {
    let ram = RAM.lock();
    ram.contains_key(path) || ram.contains_key(&alloc::format!("{path}/"))
}

/// Mode (with file type), owner and group as stored on disk – ext4 only; `None` for a path that is
/// not on an ext4 file system (FAT has no owners) or that the RAM layer covers.
pub fn disk_attr(path: &str) -> Option<(u32, u32, u32)> {
    if tmpfs::is_private(path) || ram_has(path) {
        return None;
    }
    let m = MOUNTS.lock();
    let (p, mnt) = m.iter().filter(|(p, _)| p == "/" || path == p || path.starts_with(&alloc::format!("{p}/"))).max_by_key(|(p, _)| p.len())?;
    match mnt {
        Mnt::Ext4(x, _) => x.lookup(&path[p.len().min(path.len())..]).map(|(_, i)| (i.mode as u32, i.uid, i.gid)),
        Mnt::Fat(_) => None,
    }
}

/// Store permission bits, owner and group on disk if `path` is a file or directory of a writable
/// ext4 file system. `false`: it is not (the caller keeps the values in memory).
pub fn set_disk_attr(path: &str, perm: u32, uid: u32, gid: u32) -> bool {
    if ram_has(path) {
        return false;
    }
    match ext4_target(path) {
        Some((i, rel)) => matches!(with_ext4(i, |x| x.set_attr(&rel, perm as u16, uid, gid)), Some(Ok(()))),
        None => false,
    }
}

static WARNED: Mutex<bool> = Mutex::new(false);

/// Datei anlegen/ersetzen: dauerhaft auf ext4, sonst in der RAM-Schicht
pub fn write_file(path: &str, data: Vec<u8>) {
    store_file(path, Arc::new(data));
}

/// Whether a file at `path` may grow to `len` bytes: the quota of the RAM tree it would be stored in
/// (a tmpfs or the host's RAM layer). Files that go to a writable disk and control files have none.
pub fn grow(path: &str, len: u64) -> bool {
    if path.starts_with("/proc/") || crate::groups::files::is_tree(path) || !tmpfs::is_private(path) && ext4_target(path).is_some() {
        return true;
    }
    tmpfs::grow(path, len)
}

/// Used and allowed bytes of the RAM tree that holds `path` (a tmpfs, or the host's RAM layer for a
/// path that is not on a writable disk); `None` for disk files and the kernel's own trees.
pub fn ram_usage(path: &str) -> Option<(u64, u64)> {
    if path.starts_with("/proc") || path.starts_with("/dev") || crate::groups::files::is_tree(path) || !tmpfs::is_private(path) && ext4_target(path).is_some() {
        return None;
    }
    // on the FAT test image and without a disk everything written lives in the RAM layer
    tmpfs::usage(path)
}

/// Like [`write_file`] with a buffer that may be shared (it is stored as it is, without a copy).
/// `false`: not stored – see [`store_file_res`].
pub fn store_file(path: &str, data: Arc<Vec<u8>>) -> bool {
    store_file_res(path, data).is_ok()
}

/// Stores a whole file. On a writable ext4 the data is on the medium when this returns `Ok`
/// (blocks written, then the device's write cache flushed); a file the ext4 writer cannot
/// place is **not** kept in memory instead: `Err(-ENOSPC)` (no space) or `Err(-EIO)`, and the
/// disk keeps the old content. RAM trees: `Err(-ENOSPC)` when their quota is exhausted.
pub fn store_file_res(path: &str, data: Arc<Vec<u8>>) -> Result<(), i64> {
    const EIO: i64 = 5;
    const ENOSPC: i64 = 28;
    let ok = |stored: bool| if stored { Ok(()) } else { Err(-ENOSPC) };
    if path == "/proc/ultar/verbund" {
        crate::verbund::proc_schreiben(&data);
        return Ok(());
    }
    if path == "/proc/ultar/firewall" {
        for z in core::str::from_utf8(&data).unwrap_or("").lines() {
            let _ = crate::firewall::befehl(z);
        }
        return Ok(());
    }
    // control files are written directly (syscall::control_write); nothing is ever stored there
    if path.starts_with("/proc/") || crate::groups::files::is_tree(path) || crate::devices::sysfs::is_tree(path) {
        return Err(-ENOSPC);
    }
    if tmpfs::is_private(path) {
        // a file of a tree that no longer exists is dropped
        return ok(tmpfs::write(path, data));
    }
    let Some((i, rel)) = ext4_target(path) else { return ok(tmpfs::store(path, data)) };
    match with_ext4(i, |x| x.write_file(&rel, &data).map(|()| x.disk)) {
        Some(Ok(disk)) => {
            tmpfs::discard(path);
            let flushed = BLOCK.lock().get_mut(disk).is_some_and(|d| d.flush().is_ok());
            if flushed { Ok(()) } else { Err(-EIO) }
        }
        Some(Err(e)) => {
            let mut w = WARNED.lock();
            if !*w {
                crate::kprintln!("[fs] {path}: {e} - not stored (the caller gets an error)");
                *w = true;
            }
            Err(if e == crate::ext4::NO_SPACE { -ENOSPC } else { -EIO })
        }
        None => Err(-EIO),
    }
}

/// Datei auf einer FAT-Partition an Ort überschreiben (Selbsttest-Protokoll); liefert den Pfad
pub fn fat_overwrite(rel: &str, data: &[u8]) -> Option<String> {
    let m = MOUNTS.lock();
    for (at, mnt) in m.iter() {
        if let Mnt::Fat(f) = mnt {
            if f.lookup(rel).is_some() && f.overwrite(rel, data) {
                return Some(alloc::format!("{}/{rel}", at.trim_end_matches('/')));
            }
        }
    }
    None
}

/// Datei auf allen FAT-Partitionen überschreiben, die sie haben; liefert die Anzahl
pub fn fat_overwrite_alle(rel: &str, data: &[u8]) -> usize {
    let m = MOUNTS.lock();
    let mut n = 0;
    for (_, mnt) in m.iter() {
        if let Mnt::Fat(f) = mnt {
            if f.lookup(rel).is_some() && f.overwrite(rel, data) {
                n += 1;
            }
        }
    }
    n
}

/// FAT-Partition mit dieser Datei vorhanden?
pub fn fat_has(rel: &str) -> bool {
    MOUNTS.lock().iter().any(|(_, mnt)| matches!(mnt, Mnt::Fat(f) if f.lookup(rel).is_some()))
}

pub fn mounts() -> Vec<(String, &'static str)> {
    MOUNTS.lock().iter().map(|(p, m)| (p.clone(), match m {
        Mnt::Fat(_) => "vfat",
        Mnt::Ext4(_, true) => "ext4",
        Mnt::Ext4(_, false) => "ext4 (nur lesen)",
    })).collect()
}

pub fn mkdir(path: &str) -> bool {
    if crate::devices::sysfs::is_tree(path) {
        return false;
    }
    if tmpfs::is_private(path) {
        return tmpfs::mkdir(path);
    }
    if crate::groups::files::is_tree(path) {
        return crate::groups::files::mkdir(path);
    }
    if stat(path).is_some() {
        return false;
    }
    if let Some((i, rel)) = ext4_target(path) {
        if let Some(Ok(())) = with_ext4(i, |x| x.mkdir(&rel)) {
            return true;
        }
    }
    tmpfs::store(&alloc::format!("{path}/"), Arc::new(Vec::new()))
}

pub fn unlink(path: &str) -> bool {
    if crate::devices::sysfs::is_tree(path) {
        return false;
    }
    if tmpfs::is_private(path) {
        return tmpfs::unlink(path);
    }
    if crate::groups::files::is_tree(path) {
        return crate::groups::files::rmdir(path);
    }
    if tmpfs::discard(path) || tmpfs::discard(&alloc::format!("{path}/")) {
        return true;
    }
    if let Some((i, rel)) = ext4_target(path) {
        if let Some(Ok(())) = with_ext4(i, |x| x.unlink(&rel)) {
            return true;
        }
    }
    false
}

/// ganzen Inhalt einer Datei lesen
pub fn read_all(path: &str) -> Option<Arc<Vec<u8>>> {
    match open(path)? {
        Node::File(d) => Some(d),
        _ => None,
    }
}

/// Target of the symbolic link at `path` on a disk file system, without following it (`None` if the
/// path is not a link). Parent directories must already be free of links (the path walker ensures that).
pub fn disk_link(path: &str) -> Option<String> {
    let m = MOUNTS.lock();
    let best = m.iter().filter(|(p, _)| p == "/" || path == p || path.starts_with(&alloc::format!("{p}/"))).max_by_key(|(p, _)| p.len())?;
    match &best.1 {
        Mnt::Ext4(x, _) => x.link_at(&path[best.0.len().min(path.len())..]),
        Mnt::Fat(_) => None,
    }
}
