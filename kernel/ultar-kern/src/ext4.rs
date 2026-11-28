//! ext4 lesen (eigene Umsetzung nach der offenen Beschreibung des Formats): Superblock, Gruppendeskriptoren
//! (32/64 Bit), Inodes, Extent-Bäume und klassische Blockzeiger, Verzeichnisse (linear, auch HTree),
//! symbolische Verknüpfungen. Damit liest der Ultar-Kern die Ubuntu-Partition `ultar-compat` ohne Linux.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub struct Ext4 {
    pub disk: usize,
    pub part: u64,
    bs: u64,
    ipg: u64,
    isize: u64,
    dsize: u64,
    gdt: u64,
    /// Group descriptor blocks read so far (disk block, content): every inode lookup and every
    /// allocation needs one, so they are read from the disk only once.
    gd_cache: spin::Mutex<Vec<(u64, Vec<u8>)>>,
}

/// At most this many descriptor blocks stay cached (256 × 4 KiB = 1 MiB, 16384 groups at 64 bytes).
const GD_CACHE_MAX: usize = 256;

/// Where a file lies on the disk.
pub struct Layout {
    /// Size of the file in bytes.
    pub size: u64,
    /// Runs of blocks, sorted: first block in the file, first block on the disk, number of blocks.
    pub runs: Vec<(u64, u64, u64)>,
}

pub struct Inode {
    pub mode: u16,
    pub size: u64,
    pub flags: u32,
    block: [u8; 60],
    #[allow(dead_code)] // nothing uses it yet; kept, see docs/OFFEN-WARNUNGEN.md
    pub mtime: u32,
    /// Owner (32 bits: low half at 2, high half at 0x78) and group (24, 0x7A).
    pub uid: u32,
    pub gid: u32,
}

impl Inode {
    pub fn is_dir(&self) -> bool {
        self.mode & 0xF000 == 0x4000
    }
    pub fn is_link(&self) -> bool {
        self.mode & 0xF000 == 0xA000
    }
}

fn le16(b: &[u8], o: usize) -> u64 {
    u16::from_le_bytes([b[o], b[o + 1]]) as u64
}
fn le32(b: &[u8], o: usize) -> u64 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as u64
}

fn rd(disk: usize, lba: u64, bytes: usize) -> Option<Vec<u8>> {
    let mut b = crate::mem::try_vec(bytes.div_ceil(512) * 512)?;
    let mut bl = crate::driver::BLOCK.lock();
    crate::driver::read_dev(&mut bl, disk, lba, &mut b).ok()?;
    Some(b)
}

impl Ext4 {
    pub fn probe(disk: usize, part: u64) -> Option<Ext4> {
        let sb = rd(disk, part + 2, 1024)?;
        if le16(&sb, 56) != 0xEF53 {
            return None;
        }
        let bs = 1024u64 << le32(&sb, 24);
        let incompat = le32(&sb, 96);
        let dsize = if incompat & 0x80 != 0 { le16(&sb, 254).max(32) } else { 32 };
        let first = le32(&sb, 20);
        let isize = if le32(&sb, 76) >= 1 { le16(&sb, 88) } else { 128 };
        Some(Ext4 { disk, part, bs, ipg: le32(&sb, 40), isize, dsize, gdt: first + 1, gd_cache: spin::Mutex::new(Vec::new()) })
    }

    fn read_blocks(&self, b: u64, n: u64) -> Option<Vec<u8>> {
        rd(self.disk, self.part + b * self.bs / 512, (n * self.bs) as usize)
    }

    /// A group descriptor block, from the cache when it was read before.
    fn gd_block(&self, b: u64) -> Option<Vec<u8>> {
        if let Some((_, d)) = self.gd_cache.lock().iter().find(|(n, _)| *n == b) {
            return Some(d.clone());
        }
        let d = self.read_blocks(b, 1)?;
        let mut c = self.gd_cache.lock();
        if c.len() < GD_CACHE_MAX {
            c.push((b, d.clone()));
        }
        Some(d)
    }

    /// Keeps the cache equal to the disk after `data` was written from block `b` on.
    fn gd_written(&self, b: u64, data: &[u8]) {
        let mut c = self.gd_cache.lock();
        for (n, d) in c.iter_mut() {
            if *n >= b && (*n - b) * self.bs < data.len() as u64 {
                let o = ((*n - b) * self.bs) as usize;
                let len = d.len().min(data.len() - o);
                d[..len].copy_from_slice(&data[o..o + len]);
            }
        }
    }

    /// `n` blocks from disk block `b` on (for [`crate::bigfile`]).
    pub fn blocks(&self, b: u64, n: u64) -> Option<Vec<u8>> {
        self.read_blocks(b, n)
    }

    /// Where the regular file at `path` lies on the disk, if it has at least `min` bytes and this
    /// file system has 4 KiB blocks and keeps the file in extents – so that parts of it can be
    /// read without reading all of it.
    pub fn layout(&self, path: &str, min: u64) -> Option<Layout> {
        let (_, ino) = self.lookup(path)?;
        let regular = ino.mode & 0xf000 == 0x8000;
        if !regular || ino.size < min || self.bs != 4096 || ino.flags & 0x80000 == 0 || ino.flags & 0x1000_0000 != 0 {
            return None;
        }
        let mut found = Vec::new();
        self.extents(&ino.block, &mut found, 0);
        let mut runs: Vec<(u64, u64, u64)> = found.into_iter().filter(|e| !e.3).map(|e| (e.0, e.1, e.2)).collect();
        runs.sort_unstable();
        Some(Layout { size: ino.size, runs })
    }

    pub fn inode(&self, ino: u64) -> Option<Inode> {
        let g = (ino - 1) / self.ipg;
        let idx = (ino - 1) % self.ipg;
        let doff = g * self.dsize;
        let d = self.gd_block(self.gdt + doff / self.bs)?;
        let o = (doff % self.bs) as usize;
        let mut table = le32(&d, o + 8);
        if self.dsize >= 64 {
            table |= le32(&d, o + 0x28) << 32;
        }
        let ioff = idx * self.isize;
        let b = self.read_blocks(table + ioff / self.bs, 1)?;
        let i = &b[(ioff % self.bs) as usize..];
        let mut block = [0u8; 60];
        block.copy_from_slice(&i[40..100]);
        let (uid, gid) = ((le16(i, 2) | le16(i, 0x78) << 16) as u32, (le16(i, 24) | le16(i, 0x7A) << 16) as u32);
        Some(Inode { mode: le16(i, 0) as u16, size: le32(i, 4) | le32(i, 108) << 32, flags: le32(i, 32) as u32, block, mtime: le32(i, 16) as u32, uid, gid })
    }

    /// (logischer Block, physischer Block, Anzahl, nicht initialisiert)
    fn extents(&self, node: &[u8], out: &mut Vec<(u64, u64, u64, bool)>, depth: u32) {
        if le16(node, 0) != 0xF30A || depth > 8 {
            return;
        }
        let n = le16(node, 2) as usize;
        let d = le16(node, 6);
        for k in 0..n {
            let e = &node[12 + k * 12..24 + k * 12];
            if d == 0 {
                let mut len = le16(e, 4);
                let uninit = len > 32768;
                if uninit {
                    len -= 32768;
                }
                out.push((le32(e, 0), le32(e, 8) | le16(e, 6) << 32, len, uninit));
            } else {
                let leaf = le32(e, 4) | le16(e, 8) << 32;
                if let Some(b) = self.read_blocks(leaf, 1) {
                    self.extents(&b, out, depth + 1);
                }
            }
        }
    }

    fn indirect(&self, blk: u64, level: u32, logical: &mut u64, max: u64, out: &mut Vec<(u64, u64, u64, bool)>) {
        let per = self.bs / 4;
        if blk == 0 {
            *logical += per.pow(level);
            return;
        }
        let Some(b) = self.read_blocks(blk, 1) else { return };
        for k in 0..per as usize {
            if *logical >= max {
                return;
            }
            let p = le32(&b, k * 4);
            if level == 1 {
                if p != 0 {
                    out.push((*logical, p, 1, false));
                }
                *logical += 1;
            } else {
                self.indirect(p, level - 1, logical, max, out);
            }
        }
    }

    /// ganzen Inhalt lesen (höchstens `limit` Bytes)
    pub fn read(&self, ino: &Inode, limit: u64) -> Vec<u8> {
        let size = ino.size.min(limit) as usize;
        // the whole file is read into memory: without memory for it the file reads as empty
        let Some(mut out) = crate::mem::try_vec(size) else { return Vec::new() };
        if ino.flags & 0x1000_0000 != 0 {
            let n = size.min(60);
            out[..n].copy_from_slice(&ino.block[..n]);
            return out;
        }
        let mut ex = Vec::new();
        if ino.flags & 0x80000 != 0 {
            self.extents(&ino.block, &mut ex, 0);
        } else {
            let nblocks = (size as u64).div_ceil(self.bs);
            for k in 0..12u64 {
                let p = le32(&ino.block, k as usize * 4);
                if p != 0 && k < nblocks {
                    ex.push((k, p, 1, false));
                }
            }
            let mut l = 12;
            for (lvl, idx) in [(1u32, 12usize), (2, 13), (3, 14)] {
                if l < nblocks {
                    self.indirect(le32(&ino.block, idx * 4), lvl, &mut l, nblocks, &mut ex);
                }
            }
        }
        for (lb, pb, len, uninit) in ex {
            let start = (lb * self.bs) as usize;
            if uninit || start >= size {
                continue;
            }
            // in Stücken zu höchstens 4 MB lesen
            let mut done = 0u64;
            while done < len {
                let n = (len - done).min((4 << 20) / self.bs);
                let s = start + (done * self.bs) as usize;
                if s >= size {
                    break;
                }
                if let Some(b) = self.read_blocks(pb + done, n) {
                    let m = b.len().min(size - s);
                    out[s..s + m].copy_from_slice(&b[..m]);
                }
                done += n;
            }
        }
        out
    }

    /// Verzeichniseinträge: (Name, Inode-Nummer, Typ)
    pub fn list(&self, dir: &Inode) -> Vec<(String, u64, u8)> {
        let data = self.read(dir, 64 << 20);
        let mut out = Vec::new();
        let mut o = 0usize;
        while o + 8 <= data.len() {
            let ino = le32(&data, o);
            let rec = le16(&data, o + 4) as usize;
            let nl = data[o + 6] as usize;
            let t = data[o + 7];
            if rec < 8 {
                break;
            }
            if ino != 0 && o + 8 + nl <= data.len() {
                let name = String::from_utf8_lossy(&data[o + 8..o + 8 + nl]).into_owned();
                if name != "." && name != ".." {
                    out.push((name, ino, t));
                }
            }
            o += rec;
        }
        out
    }

    fn link_target(&self, ino: &Inode) -> String {
        let d = if ino.size < 60 && ino.flags & 0x80000 == 0 { ino.block[..ino.size as usize].to_vec() } else { self.read(ino, 4096) };
        String::from_utf8_lossy(&d).into_owned()
    }

    /// Target of the symbolic link at `path` (relative to this file system's root) without following
    /// it; `None` if the last component is not a link.
    pub fn link_at(&self, path: &str) -> Option<String> {
        let path = path.trim_matches('/');
        let (dir, name) = match path.rfind('/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path),
        };
        if name.is_empty() {
            return None;
        }
        let (_, parent) = self.lookup(dir)?;
        if !parent.is_dir() {
            return None;
        }
        let entry = self.list(&parent).into_iter().find(|e| e.0 == name)?;
        let node = self.inode(entry.1)?;
        node.is_link().then(|| self.link_target(&node))
    }

    /// Pfad (relativ zur Wurzel dieses Dateisystems) auflösen, Verknüpfungen folgen
    pub fn lookup(&self, path: &str) -> Option<(u64, Inode)> {
        let mut comps: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(String::from).collect();
        comps.reverse();
        let mut stack: Vec<u64> = vec![2];
        let mut cur = (2u64, self.inode(2)?);
        let mut links = 0;
        while let Some(c) = comps.pop() {
            if c == "." {
                continue;
            }
            if c == ".." {
                if stack.len() > 1 {
                    stack.pop();
                }
                let i = *stack.last().unwrap();
                cur = (i, self.inode(i)?);
                continue;
            }
            if !cur.1.is_dir() {
                return None;
            }
            let e = self.list(&cur.1).into_iter().find(|e| e.0 == c)?;
            let node = self.inode(e.1)?;
            if node.is_link() {
                links += 1;
                if links > 40 {
                    return None;
                }
                let t = self.link_target(&node);
                if t.starts_with('/') {
                    stack.truncate(1);
                    cur = (2, self.inode(2)?);
                }
                for p in t.split('/').filter(|s| !s.is_empty()).rev() {
                    comps.push(String::from(p));
                }
                continue;
            }
            stack.push(e.1);
            cur = (e.1, node);
        }
        Some(cur)
    }
}

// ------------------------------------------------------------------ Schreiben
//
// Ohne Journal, aber mit allen Prüfsummen (metadata_csum): Superblock, Gruppendeskriptoren, Bitmaps,
// Inodes, Verzeichnisblöcke. Dateien bekommen bis zu 4 Extents direkt im Inode. Verzeichnisse mit
// HTree-Index werden nicht verändert (dann bleibt die Datei in der RAM-Schicht).

static CRC_TAB: spin::Once<[u32; 256]> = spin::Once::new();

fn crc32c(mut crc: u32, data: &[u8]) -> u32 {
    let t = CRC_TAB.call_once(|| {
        let mut t = [0u32; 256];
        for i in 0..256u32 {
            let mut c = i;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0x82F6_3B78 } else { c >> 1 };
            }
            t[i as usize] = c;
        }
        t
    });
    for &b in data {
        crc = t[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc
}

fn put16(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes());
}
fn put32(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 4].copy_from_slice(&(v as u32).to_le_bytes());
}

fn wr(disk: usize, lba: u64, data: &[u8]) -> Option<()> {
    let mut bl = crate::driver::BLOCK.lock();
    crate::driver::write_dev(&mut bl, disk, lba, data).ok()
}

pub type WResult<T> = core::result::Result<T, &'static str>;
/// Error text of a full file system (the caller maps it to `ENOSPC`).
pub const NO_SPACE: &str = "Datenträger voll";

impl Ext4 {
    fn sb(&self) -> Option<Vec<u8>> {
        rd(self.disk, self.part + 2, 1024)
    }
    fn write_sb(&self, sb: &mut [u8]) -> Option<()> {
        if self.csum(sb) {
            let c = crc32c(!0, &sb[..0x3FC]);
            put32(sb, 0x3FC, c as u64);
        }
        wr(self.disk, self.part + 2, sb)
    }
    fn csum(&self, sb: &[u8]) -> bool {
        le32(sb, 100) & 0x400 != 0
    }
    fn seed(&self, sb: &[u8]) -> u32 {
        if le32(sb, 96) & 0x2000 != 0 {
            le32(sb, 0x270) as u32
        } else {
            crc32c(!0, &sb[104..120])
        }
    }
    fn block(&self, b: u64) -> WResult<Vec<u8>> {
        self.read_blocks(b, 1).ok_or("Lesefehler")
    }
    fn put_block(&self, b: u64, data: &[u8]) -> WResult<()> {
        wr(self.disk, self.part + b * self.bs / 512, data).ok_or("Schreibfehler")?;
        self.gd_written(b, data);
        Ok(())
    }
    fn groups(&self, sb: &[u8]) -> u64 {
        let blocks = le32(sb, 4) | if le32(sb, 96) & 0x80 != 0 { le32(sb, 0x150) << 32 } else { 0 };
        (blocks - le32(sb, 20)).div_ceil(le32(sb, 32))
    }
    /// Gruppendeskriptor lesen: (Blocknummer, Versatz, Block)
    fn gd(&self, g: u64) -> WResult<(u64, usize, Vec<u8>)> {
        let off = g * self.dsize;
        let b = self.gdt + off / self.bs;
        Ok((b, (off % self.bs) as usize, self.gd_block(b).ok_or("Lesefehler")?))
    }
    fn gd_field(d: &[u8], o: usize, lo: usize, hi: usize, wide: bool, bits: u32) -> u64 {
        let l = if bits == 16 { le16(d, o + lo) } else { le32(d, o + lo) };
        if wide {
            l | (if bits == 16 { le16(d, o + hi) } else { le32(d, o + hi) }) << bits
        } else {
            l
        }
    }
    fn gd_csum(&self, sb: &[u8], g: u64, blk: &mut [u8], o: usize) {
        if !self.csum(sb) {
            return;
        }
        let ds = self.dsize as usize;
        let mut d = blk[o..o + ds].to_vec();
        d[0x1E] = 0;
        d[0x1F] = 0;
        let c = crc32c(crc32c(self.seed(sb), &(g as u32).to_le_bytes()), &d);
        put16(blk, o + 0x1E, (c & 0xffff) as u64);
    }

    /// `n` Blöcke belegen (möglichst zusammenhängend): Liste (Start, Länge)
    /// Whether group `g` holds a superblock backup (sparse_super: 0, 1 and powers of 3, 5, 7).
    fn has_super(sb: &[u8], g: u64) -> bool {
        if le32(sb, 100) & 1 == 0 || g <= 1 {
            return true;
        }
        [3u64, 5, 7].iter().any(|&b| {
            let mut p = b;
            while p < g {
                p *= b;
            }
            p == g
        })
    }

    /// The block bitmap of a group that is still BLOCK_UNINIT, as the kernel and e2fsck derive it:
    /// superblock backup and descriptor blocks (with the reserved ones), every bitmap and inode
    /// table of any group that lies in this group (flex_bg), and the bits past the group's end.
    /// `None` for layouts this writer does not derive (meta_bg, sparse_super2).
    fn init_block_bitmap(&self, sb: &[u8], g: u64) -> WResult<Option<Vec<u8>>> {
        if le32(sb, 96) & 0x10 != 0 || le32(sb, 92) & 0x200 != 0 {
            return Ok(None);
        }
        let (bpg, first, wide) = (le32(sb, 32), le32(sb, 20), self.dsize >= 64);
        let groups = self.groups(sb);
        let start = first + g * bpg;
        let total = le32(sb, 4) | if le32(sb, 96) & 0x80 != 0 { le32(sb, 0x150) << 32 } else { 0 };
        let in_group = bpg.min(total - start);
        let mut bm = alloc::vec![0u8; self.bs as usize];
        let mut mark = |b: u64| {
            if (start..start + in_group).contains(&b) {
                let i = b - start;
                bm[(i / 8) as usize] |= 1 << (i % 8);
            }
        };
        if Self::has_super(sb, g) {
            let gdt = (groups * self.dsize).div_ceil(self.bs) + le16(sb, 0xCE);
            (start..start + 1 + gdt).for_each(&mut mark);
        }
        let itable = (le32(sb, 40) * self.isize).div_ceil(self.bs);
        for other in 0..groups {
            let (_, o, d) = self.gd(other)?;
            mark(Self::gd_field(&d, o, 0, 0x20, wide, 32));
            mark(Self::gd_field(&d, o, 4, 0x24, wide, 32));
            let it = Self::gd_field(&d, o, 8, 0x28, wide, 32);
            (it..it + itable).for_each(&mut mark);
        }
        for i in in_group..self.bs * 8 {
            bm[(i / 8) as usize] |= 1 << (i % 8);
        }
        Ok(Some(bm))
    }

    fn alloc_blocks(&self, mut n: u64) -> WResult<Vec<(u64, u64)>> {
        let mut sb = self.sb().ok_or("Lesefehler")?;
        let bpg = le32(&sb, 32);
        let first = le32(&sb, 20);
        let wide = self.dsize >= 64;
        let mut runs: Vec<(u64, u64)> = Vec::new();
        let total = n;
        for g in 0..self.groups(&sb) {
            if n == 0 {
                break;
            }
            let (db, o, mut dblk) = self.gd(g)?;
            let free = Self::gd_field(&dblk, o, 12, 0x2C, wide, 16);
            if free == 0 {
                continue;
            }
            let bmb = Self::gd_field(&dblk, o, 0, 0x20, wide, 32);
            // BLOCK_UNINIT: the bitmap on disk was never written; build it (written below, with
            // the flag cleared, once a block of the group is taken)
            let uninit = le16(&dblk, o + 18) & 2 != 0;
            let mut bm = if uninit {
                match self.init_block_bitmap(&sb, g)? {
                    Some(b) => b,
                    None => continue,
                }
            } else {
                self.block(bmb)?
            };
            if uninit {
                let flags = le16(&dblk, o + 18) & !2;
                put16(&mut dblk, o + 18, flags);
            }
            let mut taken = 0u64;
            let mut i = 0u64;
            while i < bpg && n > 0 {
                if bm[(i / 8) as usize] & (1 << (i % 8)) == 0 {
                    let start = i;
                    while i < bpg && n > 0 && bm[(i / 8) as usize] & (1 << (i % 8)) == 0 && i - start < 32768 {
                        bm[(i / 8) as usize] |= 1 << (i % 8);
                        i += 1;
                        n -= 1;
                        taken += 1;
                    }
                    let abs = first + g * bpg + start;
                    match runs.last_mut() {
                        Some(l) if l.0 + l.1 == abs && l.1 + (i - start) <= 32768 => l.1 += i - start,
                        _ => runs.push((abs, i - start)),
                    }
                } else {
                    i += 1;
                }
            }
            if taken == 0 {
                continue;
            }
            self.put_block(bmb, &bm)?;
            let nf = free - taken;
            put16(&mut dblk, o + 12, nf & 0xffff);
            if wide {
                put16(&mut dblk, o + 0x2C, nf >> 16);
            }
            if self.csum(&sb) {
                let c = crc32c(self.seed(&sb), &bm[..(bpg / 8) as usize]);
                put16(&mut dblk, o + 24, (c & 0xffff) as u64);
                if wide {
                    put16(&mut dblk, o + 0x38, (c >> 16) as u64);
                }
            }
            self.gd_csum(&sb, g, &mut dblk, o);
            self.put_block(db, &dblk)?;
        }
        if n > 0 {
            // zurückgeben, was schon belegt wurde: the superblock counts it as taken first, so that
            // giving it back does not leave it counted free twice (statfs showed room that was not there)
            let fb = le32(&sb, 12) | le32(&sb, 0x158) << 32;
            let nf = fb.saturating_sub(total - n);
            put32(&mut sb, 12, nf);
            put32(&mut sb, 0x158, nf >> 32);
            let _ = self.write_sb(&mut sb);
            let _ = self.free_runs(&runs);
            return Err(NO_SPACE);
        }
        let fb = le32(&sb, 12) | le32(&sb, 0x158) << 32;
        let nf = fb.saturating_sub(total);
        put32(&mut sb, 12, nf);
        put32(&mut sb, 0x158, nf >> 32);
        self.write_sb(&mut sb).ok_or("Schreibfehler")?;
        Ok(runs)
    }

    fn free_runs(&self, runs: &[(u64, u64)]) -> WResult<()> {
        let mut sb = self.sb().ok_or("Lesefehler")?;
        let bpg = le32(&sb, 32);
        let first = le32(&sb, 20);
        let wide = self.dsize >= 64;
        let mut freed = 0u64;
        for &(start, len) in runs {
            let mut b = start;
            while b < start + len {
                let g = (b - first) / bpg;
                let (db, o, mut dblk) = self.gd(g)?;
                let bmb = Self::gd_field(&dblk, o, 0, 0x20, wide, 32);
                let mut bm = self.block(bmb)?;
                let mut k = 0;
                while b < start + len && (b - first) / bpg == g {
                    let i = (b - first) % bpg;
                    if bm[(i / 8) as usize] & (1 << (i % 8)) != 0 {
                        bm[(i / 8) as usize] &= !(1 << (i % 8));
                        k += 1;
                    }
                    b += 1;
                }
                self.put_block(bmb, &bm)?;
                let nf = Self::gd_field(&dblk, o, 12, 0x2C, wide, 16) + k;
                put16(&mut dblk, o + 12, nf & 0xffff);
                if wide {
                    put16(&mut dblk, o + 0x2C, nf >> 16);
                }
                if self.csum(&sb) {
                    let c = crc32c(self.seed(&sb), &bm[..(bpg / 8) as usize]);
                    put16(&mut dblk, o + 24, (c & 0xffff) as u64);
                    if wide {
                        put16(&mut dblk, o + 0x38, (c >> 16) as u64);
                    }
                }
                self.gd_csum(&sb, g, &mut dblk, o);
                self.put_block(db, &dblk)?;
                freed += k;
            }
        }
        let fb = (le32(&sb, 12) | le32(&sb, 0x158) << 32) + freed;
        put32(&mut sb, 12, fb);
        put32(&mut sb, 0x158, fb >> 32);
        self.write_sb(&mut sb).ok_or("Schreibfehler")?;
        Ok(())
    }

    /// Inode belegen (`dir`: Verzeichnis zählen)
    fn alloc_inode(&self, dir: bool) -> WResult<u64> {
        let mut sb = self.sb().ok_or("Lesefehler")?;
        let ipg = self.ipg;
        let first_ino = if le32(&sb, 76) >= 1 { le32(&sb, 84) } else { 11 };
        let wide = self.dsize >= 64;
        for g in 0..self.groups(&sb) {
            let (db, o, mut dblk) = self.gd(g)?;
            let flags = le16(&dblk, o + 18);
            let free = Self::gd_field(&dblk, o, 14, 0x2E, wide, 16);
            if free == 0 {
                continue;
            }
            let bmb = Self::gd_field(&dblk, o, 4, 0x24, wide, 32);
            // INODE_UNINIT (a group a growth added): the bitmap on disk was never written – all
            // inodes free, the bits past the group's inodes set; written below with the flag
            // cleared. The inode table is not zeroed: every inode below `itable_unused` is
            // written whole before it is used (new_inode).
            let mut bm = if flags & 1 != 0 {
                let mut b = alloc::vec![0u8; self.bs as usize];
                for i in ipg..self.bs * 8 {
                    b[(i / 8) as usize] |= 1 << (i % 8);
                }
                put16(&mut dblk, o + 18, flags & !1);
                b
            } else {
                self.block(bmb)?
            };
            for i in 0..ipg {
                let ino = g * ipg + i + 1;
                if ino < first_ino || bm[(i / 8) as usize] & (1 << (i % 8)) != 0 {
                    continue;
                }
                bm[(i / 8) as usize] |= 1 << (i % 8);
                self.put_block(bmb, &bm)?;
                put16(&mut dblk, o + 14, (free - 1) & 0xffff);
                if wide {
                    put16(&mut dblk, o + 0x2E, (free - 1) >> 16);
                }
                if dir {
                    let d = Self::gd_field(&dblk, o, 16, 0x30, wide, 16) + 1;
                    put16(&mut dblk, o + 16, d & 0xffff);
                    if wide {
                        put16(&mut dblk, o + 0x30, d >> 16);
                    }
                }
                // nicht benutzte Inode-Tabelle verkleinern, falls nötig
                let unused = Self::gd_field(&dblk, o, 28, 0x32, wide, 16);
                if i + 1 > ipg - unused {
                    let nu = ipg - (i + 1);
                    put16(&mut dblk, o + 28, nu & 0xffff);
                    if wide {
                        put16(&mut dblk, o + 0x32, nu >> 16);
                    }
                }
                if self.csum(&sb) {
                    let c = crc32c(self.seed(&sb), &bm[..(ipg / 8) as usize]);
                    put16(&mut dblk, o + 26, (c & 0xffff) as u64);
                    if wide {
                        put16(&mut dblk, o + 0x3A, (c >> 16) as u64);
                    }
                }
                self.gd_csum(&sb, g, &mut dblk, o);
                self.put_block(db, &dblk)?;
                let fi = le32(&sb, 16) - 1;
                put32(&mut sb, 16, fi);
                self.write_sb(&mut sb).ok_or("Schreibfehler")?;
                return Ok(ino);
            }
        }
        Err("keine freien Inodes")
    }

    fn free_inode(&self, ino: u64, dir: bool) -> WResult<()> {
        let mut sb = self.sb().ok_or("Lesefehler")?;
        let g = (ino - 1) / self.ipg;
        let i = (ino - 1) % self.ipg;
        let wide = self.dsize >= 64;
        let (db, o, mut dblk) = self.gd(g)?;
        let bmb = Self::gd_field(&dblk, o, 4, 0x24, wide, 32);
        let mut bm = self.block(bmb)?;
        bm[(i / 8) as usize] &= !(1 << (i % 8));
        self.put_block(bmb, &bm)?;
        let free = Self::gd_field(&dblk, o, 14, 0x2E, wide, 16) + 1;
        put16(&mut dblk, o + 14, free & 0xffff);
        if wide {
            put16(&mut dblk, o + 0x2E, free >> 16);
        }
        if dir {
            let d = Self::gd_field(&dblk, o, 16, 0x30, wide, 16).saturating_sub(1);
            put16(&mut dblk, o + 16, d & 0xffff);
            if wide {
                put16(&mut dblk, o + 0x30, d >> 16);
            }
        }
        if self.csum(&sb) {
            let c = crc32c(self.seed(&sb), &bm[..(self.ipg / 8) as usize]);
            put16(&mut dblk, o + 26, (c & 0xffff) as u64);
            if wide {
                put16(&mut dblk, o + 0x3A, (c >> 16) as u64);
            }
        }
        self.gd_csum(&sb, g, &mut dblk, o);
        self.put_block(db, &dblk)?;
        let fi = le32(&sb, 16) + 1;
        put32(&mut sb, 16, fi);
        self.write_sb(&mut sb).ok_or("Schreibfehler")?;
        Ok(())
    }

    /// rohes Inode lesen: (Block, Versatz, Block-Inhalt)
    fn raw_inode(&self, ino: u64) -> WResult<(u64, usize, Vec<u8>)> {
        let g = (ino - 1) / self.ipg;
        let idx = (ino - 1) % self.ipg;
        let (_, o, d) = self.gd(g)?;
        let mut table = le32(&d, o + 8);
        if self.dsize >= 64 {
            table |= le32(&d, o + 0x28) << 32;
        }
        let ioff = idx * self.isize;
        let b = table + ioff / self.bs;
        Ok((b, (ioff % self.bs) as usize, self.block(b)?))
    }

    fn inode_seed(&self, sb: &[u8], ino: u64, raw: &[u8]) -> u32 {
        let c = crc32c(self.seed(sb), &(ino as u32).to_le_bytes());
        crc32c(c, &raw[0x64..0x68])
    }

    /// Inode zurückschreiben (Prüfsumme neu)
    fn put_inode(&self, ino: u64, raw: &mut [u8]) -> WResult<()> {
        let sb = self.sb().ok_or("Lesefehler")?;
        let isz = self.isize as usize;
        if self.csum(&sb) {
            raw[0x7C] = 0;
            raw[0x7D] = 0;
            let hi = isz > 128 && le16(raw, 0x80) >= 4;
            if hi {
                raw[0x82] = 0;
                raw[0x83] = 0;
            }
            let c = crc32c(self.inode_seed(&sb, ino, raw), &raw[..isz]);
            put16(raw, 0x7C, (c & 0xffff) as u64);
            if hi {
                put16(raw, 0x82, (c >> 16) as u64);
            }
        }
        let (b, o, mut blk) = self.raw_inode(ino)?;
        blk[o..o + isz].copy_from_slice(&raw[..isz]);
        self.put_block(b, &blk)
    }

    fn get_raw(&self, ino: u64) -> WResult<Vec<u8>> {
        let (_, o, blk) = self.raw_inode(ino)?;
        Ok(blk[o..o + self.isize as usize].to_vec())
    }

    /// alle Blöcke eines Inodes (Daten und Extent-Baum) für die Freigabe sammeln
    fn inode_blocks(&self, raw: &[u8]) -> Vec<(u64, u64)> {
        let flags = le32(raw, 32);
        let mut out = Vec::new();
        if flags & 0x1000_0000 != 0 {
            return out;
        }
        // a fast symbolic link keeps its target in i_block and owns no block (i_blocks 0): those
        // bytes are no block pointers and must never be freed as such
        if le16(raw, 0) & 0xF000 == 0xA000 && flags & 0x80000 == 0 && le32(raw, 0x1C) == 0 {
            return out;
        }
        if flags & 0x80000 != 0 {
            fn walk(fs: &Ext4, node: &[u8], out: &mut Vec<(u64, u64)>, depth: u32) {
                if le16(node, 0) != 0xF30A || depth > 8 {
                    return;
                }
                let n = le16(node, 2) as usize;
                let d = le16(node, 6);
                for k in 0..n {
                    let e = &node[12 + k * 12..24 + k * 12];
                    if d == 0 {
                        let mut len = le16(e, 4);
                        if len > 32768 {
                            len -= 32768;
                        }
                        out.push((le32(e, 8) | le16(e, 6) << 32, len));
                    } else {
                        let leaf = le32(e, 4) | le16(e, 8) << 32;
                        out.push((leaf, 1));
                        if let Some(b) = fs.read_blocks(leaf, 1) {
                            walk(fs, &b, out, depth + 1);
                        }
                    }
                }
            }
            walk(self, &raw[40..100], &mut out, 0);
        } else {
            // klassische Blockzeiger: nur direkte Blöcke (ältere Dateien), Rest bleibt belegt
            for k in 0..12 {
                let p = le32(raw, 40 + k * 4);
                if p != 0 {
                    out.push((p, 1));
                }
            }
        }
        out
    }

    /// Extents per leaf block of an extent tree (the 4-byte checksum tail stays free).
    fn leaf_capacity(&self) -> usize {
        (self.bs as usize - 12 - 4) / 12
    }

    /// Writes an extent node: header (magic, entries, `max`, `depth`) and entries. Leaf entries
    /// map logical blocks to `runs` (from logical block `first`); index entries (depth 1) point to
    /// the leaf blocks in `runs` (start = leaf block, len = extents in that leaf).
    fn extent_node(node: &mut [u8], runs: &[(u64, u64)], first: u64, max: usize, depth: u64) {
        put16(node, 0, 0xF30A);
        put16(node, 2, runs.len() as u64);
        put16(node, 4, max as u64);
        put16(node, 6, depth);
        let mut logical = first;
        for (k, &(start, len)) in runs.iter().enumerate() {
            let o = 12 + k * 12;
            put32(node, o, logical);
            if depth == 0 {
                put16(node, o + 4, len);
                put16(node, o + 6, start >> 32);
                put32(node, o + 8, start);
            } else {
                put32(node, o + 4, start);
                put16(node, o + 8, start >> 32);
            }
            logical += if depth == 0 { len } else { 0 };
        }
    }

    /// The 60-byte extent root of the inode for `runs`, and the leaf blocks it needs. Up to 4
    /// runs fit in the inode itself (depth 0); more get a tree of depth 1 with up to 4 leaf
    /// blocks (`4 × leaf_capacity` runs). Leaves are written (with checksum) before returning.
    fn extent_tree(&self, ino: u64, raw: &[u8], runs: &[(u64, u64)]) -> WResult<([u8; 60], Vec<(u64, u64)>)> {
        let mut root = [0u8; 60];
        if runs.len() <= 4 {
            Self::extent_node(&mut root, runs, 0, 4, 0);
            return Ok((root, Vec::new()));
        }
        let per = self.leaf_capacity();
        let leaves = runs.len().div_ceil(per);
        if leaves > 4 {
            return Err("Datei zu zerstückelt");
        }
        let blocks: Vec<u64> = self.alloc_blocks(leaves as u64)?.iter().flat_map(|&(s, l)| s..s + l).collect();
        let sb = self.sb().ok_or("Lesefehler")?;
        let mut index = Vec::new();
        let mut logical = 0u64;
        for (chunk, &leaf) in runs.chunks(per).zip(&blocks) {
            let mut blk = alloc::vec![0u8; self.bs as usize];
            Self::extent_node(&mut blk, chunk, logical, per, 0);
            let tail = 12 + 12 * per;
            if self.csum(&sb) {
                let c = crc32c(self.inode_seed(&sb, ino, raw), &blk[..tail]);
                put32(&mut blk, tail, c as u64);
            }
            if let Err(e) = self.put_block(leaf, &blk) {
                let _ = self.free_runs(&blocks.iter().map(|&b| (b, 1)).collect::<Vec<_>>());
                return Err(e);
            }
            index.push((leaf, logical));
            logical += chunk.iter().map(|r| r.1).sum::<u64>();
        }
        put16(&mut root, 0, 0xF30A);
        put16(&mut root, 2, index.len() as u64);
        put16(&mut root, 4, 4);
        put16(&mut root, 6, 1);
        for (k, &(leaf, first)) in index.iter().enumerate() {
            let o = 12 + k * 12;
            put32(&mut root, o, first);
            put32(&mut root, o + 4, leaf);
            put16(&mut root, o + 8, leaf >> 32);
        }
        Ok((root, blocks.iter().map(|&b| (b, 1)).collect()))
    }

    /// Writes `data` into freshly allocated blocks; returns the runs.
    fn write_runs(&self, data: &[u8]) -> WResult<Vec<(u64, u64)>> {
        let nb = (data.len() as u64).div_ceil(self.bs);
        let runs = if nb > 0 { self.alloc_blocks(nb)? } else { Vec::new() };
        let mut logical = 0u64;
        for &(start, len) in &runs {
            let s = (logical * self.bs) as usize;
            let e = ((logical + len) * self.bs) as usize;
            let mut chunk = data[s.min(data.len())..e.min(data.len())].to_vec();
            chunk.resize((len * self.bs) as usize, 0);
            if let Err(e) = self.put_block_n(start, &chunk) {
                let _ = self.free_runs(&runs);
                return Err(e);
            }
            logical += len;
        }
        Ok(runs)
    }

    /// Data runs (in file order) and extent tree blocks of an extent inode; `None` for other inodes.
    fn data_and_tree(&self, raw: &[u8]) -> Option<(Vec<(u64, u64)>, Vec<(u64, u64)>)> {
        if le32(raw, 32) & 0x80000 == 0 {
            return None;
        }
        fn walk(fs: &Ext4, node: &[u8], data: &mut Vec<(u64, u64)>, tree: &mut Vec<(u64, u64)>, depth: u32) -> Option<()> {
            if le16(node, 0) != 0xF30A || depth > 8 {
                return None;
            }
            let d = le16(node, 6);
            for k in 0..le16(node, 2) as usize {
                let e = node.get(12 + k * 12..24 + k * 12)?;
                if d == 0 {
                    let len = le16(e, 4);
                    data.push((le32(e, 8) | le16(e, 6) << 32, if len > 32768 { len - 32768 } else { len }));
                } else {
                    let leaf = le32(e, 4) | le16(e, 8) << 32;
                    tree.push((leaf, 1));
                    walk(fs, &fs.read_blocks(leaf, 1)?, data, tree, depth + 1)?;
                }
            }
            Some(())
        }
        let (mut data, mut tree) = (Vec::new(), Vec::new());
        walk(self, &raw[40..100], &mut data, &mut tree, 0)?;
        Some((data, tree))
    }

    /// Like [`Ext4::set_data`] when the free space cannot hold a second copy of the file: the data goes
    /// into the file's own blocks (more are taken only for growth, the rest is freed). A stop while it
    /// writes leaves a mix of old and new bytes – the file system itself stays consistent (the inode
    /// changes last). Only used when the copy found no room (a file larger than the free space could
    /// otherwise never be changed again: `ENOSPC` on a half-empty disk).
    fn set_data_in_place(&self, ino: u64, raw: &mut [u8], data: &[u8]) -> WResult<()> {
        let Some((old, tree)) = self.data_and_tree(raw) else { return Err(NO_SPACE) };
        let need = (data.len() as u64).div_ceil(self.bs);
        let have: u64 = old.iter().map(|r| r.1).sum();
        // growth first: without room for it nothing has changed yet
        let extra = if need > have { self.alloc_blocks(need - have)? } else { Vec::new() };
        let (mut runs, mut tail) = (Vec::new(), Vec::new());
        let mut left = need;
        for &(start, len) in &old {
            let take = len.min(left);
            if take > 0 {
                runs.push((start, take));
            }
            if take < len {
                tail.push((start + take, len - take));
            }
            left -= take;
        }
        runs.extend_from_slice(&extra);
        let mut logical = 0u64;
        for &(start, len) in &runs {
            let s = ((logical * self.bs) as usize).min(data.len());
            let e = (((logical + len) * self.bs) as usize).min(data.len());
            let mut chunk = data[s..e].to_vec();
            chunk.resize((len * self.bs) as usize, 0);
            self.put_block_n(start, &chunk)?;
            logical += len;
        }
        let (root, leaves) = match self.extent_tree(ino, raw, &runs) {
            Ok(t) => t,
            Err(e) => {
                let _ = self.free_runs(&extra);
                return Err(e);
            }
        };
        raw[40..100].copy_from_slice(&root);
        let flags = (le32(raw, 32) | 0x80000) & !0x1000;
        put32(raw, 32, flags);
        put32(raw, 4, data.len() as u64);
        put32(raw, 108, (data.len() as u64) >> 32);
        let sectors = (need + leaves.len() as u64) * self.bs / 512;
        put32(raw, 0x1C, sectors);
        put16(raw, 0x74, sectors >> 32);
        let now = now_secs();
        put32(raw, 0x0C, now);
        put32(raw, 0x10, now);
        self.put_inode(ino, raw)?;
        tail.extend_from_slice(&tree);
        if !tail.is_empty() {
            self.free_runs(&tail)?;
        }
        Ok(())
    }

    /// Size of the file system for `statfs`: (block size, blocks, free blocks, free blocks for
    /// everyone – without the reserved ones –, inodes, free inodes).
    pub fn space(&self) -> Option<(u64, u64, u64, u64, u64, u64)> {
        let sb = self.sb()?;
        let wide = le32(&sb, 96) & 0x80 != 0;
        let hi = |o: usize| if wide { le32(&sb, o) << 32 } else { 0 };
        let blocks = le32(&sb, 4) | hi(0x150);
        let reserved = le32(&sb, 8) | hi(0x154);
        let free = le32(&sb, 12) | hi(0x158);
        Some((self.bs, blocks, free, free.saturating_sub(reserved), le32(&sb, 0), le32(&sb, 16)))
    }

    /// Inhalt eines Inodes vollständig ersetzen. Order: data blocks, extent leaves, inode, then
    /// the old blocks are freed – a stop in between leaves the old content (and leaked blocks).
    /// Without room for that copy the file's own blocks are reused ([`Ext4::set_data_in_place`]).
    fn set_data(&self, ino: u64, data: &[u8]) -> WResult<()> {
        let mut raw = self.get_raw(ino)?;
        if le32(&raw, 32) & 0x1000_0000 != 0 {
            return Err("Inline-Daten nicht unterstützt");
        }
        let old = self.inode_blocks(&raw);
        let runs = match self.write_runs(data) {
            Ok(r) => r,
            Err(e) if e == NO_SPACE && !old.is_empty() => return self.set_data_in_place(ino, &mut raw, data),
            Err(e) => return Err(e),
        };
        let (root, leaves) = match self.extent_tree(ino, &raw, &runs) {
            Ok(t) => t,
            Err(e) => {
                let _ = self.free_runs(&runs);
                return Err(e);
            }
        };
        raw[40..100].copy_from_slice(&root);
        let flags = (le32(&raw, 32) | 0x80000) & !0x1000;
        put32(&mut raw, 32, flags);
        put32(&mut raw, 4, data.len() as u64);
        put32(&mut raw, 108, (data.len() as u64) >> 32);
        let nb = (data.len() as u64).div_ceil(self.bs) + leaves.len() as u64;
        let sectors = nb * self.bs / 512;
        put32(&mut raw, 0x1C, sectors);
        put16(&mut raw, 0x74, sectors >> 32);
        let now = now_secs();
        put32(&mut raw, 0x0C, now);
        put32(&mut raw, 0x10, now);
        self.put_inode(ino, &mut raw)?;
        if !old.is_empty() {
            self.free_runs(&old)?;
        }
        Ok(())
    }

    fn put_block_n(&self, b: u64, data: &[u8]) -> WResult<()> {
        for (i, c) in data.chunks((1 << 20) as usize).enumerate() {
            let blk = b + (i as u64) * ((1 << 20) / self.bs);
            self.put_block(blk, c)?;
        }
        Ok(())
    }

    fn new_inode(&self, mode: u16, dir: bool) -> WResult<u64> {
        let ino = self.alloc_inode(dir)?;
        let mut raw = alloc::vec![0u8; self.isize as usize];
        put16(&mut raw, 0, mode as u64);
        let now = now_secs();
        for o in [8usize, 0x0C, 0x10] {
            put32(&mut raw, o, now);
        }
        put16(&mut raw, 0x1A, if dir { 2 } else { 1 });
        put32(&mut raw, 32, 0x80000);
        let mut ib = [0u8; 60];
        put16(&mut ib, 0, 0xF30A);
        put16(&mut ib, 4, 4);
        raw[40..100].copy_from_slice(&ib);
        put32(&mut raw, 0x64, crate::syscall::random_u64());
        if self.isize > 128 {
            put16(&mut raw, 0x80, 32);
            put32(&mut raw, 0x90, now);
        }
        self.put_inode(ino, &mut raw)?;
        Ok(ino)
    }

    /// Verzeichnisblöcke (logisch → physisch) eines Verzeichnisses
    fn dir_blocks(&self, ino: &Inode) -> Vec<u64> {
        let mut ex = Vec::new();
        self.extents(&ino.block, &mut ex, 0);
        let mut v = Vec::new();
        for (_, p, len, _) in ex {
            for k in 0..len {
                v.push(p + k);
            }
        }
        v
    }

    fn dir_tail_csum(&self, sb: &[u8], dino: u64, draw: &[u8], blk: &mut [u8]) {
        if !self.csum(sb) {
            return;
        }
        let bs = self.bs as usize;
        let t = bs - 12;
        if le32(blk, t) != 0 || le16(blk, t + 4) != 12 || blk[t + 7] != 0xDE {
            return;
        }
        let c = crc32c(self.inode_seed(sb, dino, draw), &blk[..t]);
        put32(blk, t + 8, c as u64);
    }

    /// Eintrag in ein Verzeichnis einfügen
    fn dir_add(&self, dino: u64, name: &str, ino: u64, ftype: u8) -> WResult<()> {
        let sb = self.sb().ok_or("Lesefehler")?;
        let draw = self.get_raw(dino)?;
        let dir = self.inode(dino).ok_or("Lesefehler")?;
        if dir.flags & 0x1000 != 0 {
            return Err("Verzeichnis mit HTree-Index");
        }
        if dir.flags & 0x80000 == 0 {
            return Err("Verzeichnis ohne Extents");
        }
        let need = (8 + name.len() + 3) & !3;
        let bs = self.bs as usize;
        let end = if self.csum(&sb) { bs - 12 } else { bs };
        for b in self.dir_blocks(&dir) {
            let mut blk = self.block(b)?;
            let mut o = 0usize;
            while o + 8 <= end {
                let e_ino = le32(&blk, o);
                let rec = le16(&blk, o + 4) as usize;
                if rec < 8 || o + rec > end + 12 {
                    break;
                }
                let used = if e_ino == 0 { 0 } else { (8 + blk[o + 6] as usize + 3) & !3 };
                if rec.saturating_sub(used) >= need {
                    let (no, nrec) = if e_ino == 0 { (o, rec) } else { (o + used, rec - used) };
                    if e_ino != 0 {
                        put16(&mut blk, o + 4, used as u64);
                    }
                    put32(&mut blk, no, ino);
                    put16(&mut blk, no + 4, nrec as u64);
                    blk[no + 6] = name.len() as u8;
                    blk[no + 7] = ftype;
                    blk[no + 8..no + 8 + name.len()].copy_from_slice(name.as_bytes());
                    self.dir_tail_csum(&sb, dino, &draw, &mut blk);
                    return self.put_block(b, &blk);
                }
                o += rec;
            }
        }
        // neuer Block für das Verzeichnis
        let mut data = self.read(&dir, 64 << 20);
        let mut nb = alloc::vec![0u8; bs];
        put32(&mut nb, 0, ino);
        put16(&mut nb, 4, end as u64);
        nb[6] = name.len() as u8;
        nb[7] = ftype;
        nb[8..8 + name.len()].copy_from_slice(name.as_bytes());
        if self.csum(&sb) {
            put16(&mut nb, bs - 12 + 4, 12);
            nb[bs - 12 + 7] = 0xDE;
        }
        data.extend_from_slice(&nb);
        self.set_data(dino, &data)?;
        // Prüfsumme des neuen (letzten) Blocks
        let dir = self.inode(dino).ok_or("Lesefehler")?;
        let draw = self.get_raw(dino)?;
        if let Some(&last) = self.dir_blocks(&dir).last() {
            let mut blk = self.block(last)?;
            self.dir_tail_csum(&sb, dino, &draw, &mut blk);
            self.put_block(last, &blk)?;
        }
        Ok(())
    }

    /// Eintrag entfernen; liefert die Inode-Nummer
    fn dir_remove(&self, dino: u64, name: &str) -> WResult<u64> {
        let sb = self.sb().ok_or("Lesefehler")?;
        let draw = self.get_raw(dino)?;
        let dir = self.inode(dino).ok_or("Lesefehler")?;
        if dir.flags & 0x1000 != 0 {
            return Err("Verzeichnis mit HTree-Index");
        }
        let bs = self.bs as usize;
        let end = if self.csum(&sb) { bs - 12 } else { bs };
        for b in self.dir_blocks(&dir) {
            let mut blk = self.block(b)?;
            let mut o = 0usize;
            let mut prev: Option<usize> = None;
            while o + 8 <= end {
                let e_ino = le32(&blk, o);
                let rec = le16(&blk, o + 4) as usize;
                if rec < 8 {
                    break;
                }
                let nl = blk[o + 6] as usize;
                if e_ino != 0 && &blk[o + 8..o + 8 + nl] == name.as_bytes() {
                    match prev {
                        Some(p) => {
                            let pr = le16(&blk, p + 4) as usize;
                            put16(&mut blk, p + 4, (pr + rec) as u64);
                        }
                        None => put32(&mut blk, o, 0),
                    }
                    self.dir_tail_csum(&sb, dino, &draw, &mut blk);
                    self.put_block(b, &blk)?;
                    return Ok(e_ino);
                }
                prev = Some(o);
                o += rec;
            }
        }
        Err("nicht gefunden")
    }

    fn split(path: &str) -> (&str, &str) {
        let p = path.trim_end_matches('/');
        match p.rfind('/') {
            Some(i) => (&p[..i], &p[i + 1..]),
            None => ("", p),
        }
    }

    /// Datei anlegen oder ersetzen
    pub fn write_file(&self, path: &str, data: &[u8]) -> WResult<()> {
        let (parent, name) = Self::split(path);
        if name.is_empty() || name.len() > 255 {
            return Err("ungültiger Name");
        }
        let (dino, d) = self.lookup(parent).ok_or("Verzeichnis fehlt")?;
        if !d.is_dir() {
            return Err("kein Verzeichnis");
        }
        let existing = self.list(&d).into_iter().find(|e| e.0 == name).map(|e| e.1);
        let ino = match existing {
            Some(i) => {
                if self.inode(i).map(|n| n.is_dir()).unwrap_or(false) {
                    return Err("ist ein Verzeichnis");
                }
                i
            }
            None => {
                let i = self.new_inode(0o100644, false)?;
                if let Err(e) = self.dir_add(dino, name, i, 1) {
                    let _ = self.free_inode(i, false);
                    return Err(e);
                }
                i
            }
        };
        self.set_data(ino, data)
    }

    /// Symbolic link at `path` to `target` (stored as is, not checked): a fast link (target in
    /// i_block, no block) below 60 bytes, else one data block like a file's.
    pub fn symlink(&self, path: &str, target: &str) -> WResult<()> {
        let (parent, name) = Self::split(path);
        if name.is_empty() || name.len() > 255 || target.is_empty() || target.len() >= self.bs as usize {
            return Err("ungültiger Name");
        }
        let (dino, d) = self.lookup(parent).ok_or("Verzeichnis fehlt")?;
        if !d.is_dir() {
            return Err("kein Verzeichnis");
        }
        if self.list(&d).iter().any(|e| e.0 == name) {
            return Err("existiert");
        }
        let ino = self.new_inode(0o120777, false)?;
        let made = if target.len() < 60 {
            self.get_raw(ino).and_then(|mut raw| {
                raw[40..100].fill(0);
                raw[40..40 + target.len()].copy_from_slice(target.as_bytes());
                let flags = le32(&raw, 32) & !0x80000;
                put32(&mut raw, 32, flags);
                put32(&mut raw, 4, target.len() as u64);
                put32(&mut raw, 108, 0);
                put32(&mut raw, 0x1C, 0);
                self.put_inode(ino, &mut raw)
            })
        } else {
            self.set_data(ino, target.as_bytes())
        };
        if let Err(e) = made.and_then(|()| self.dir_add(dino, name, ino, 7)) {
            let _ = self.free_inode(ino, false);
            return Err(e);
        }
        Ok(())
    }

    pub fn mkdir(&self, path: &str) -> WResult<()> {
        let (parent, name) = Self::split(path);
        let (dino, d) = self.lookup(parent).ok_or("Verzeichnis fehlt")?;
        if self.list(&d).iter().any(|e| e.0 == name) {
            return Err("existiert");
        }
        let ino = self.new_inode(0o040755, true)?;
        let sb = self.sb().ok_or("Lesefehler")?;
        let bs = self.bs as usize;
        let mut blk = alloc::vec![0u8; bs];
        put32(&mut blk, 0, ino);
        put16(&mut blk, 4, 12);
        blk[6] = 1;
        blk[7] = 2;
        blk[8] = b'.';
        put32(&mut blk, 12, dino);
        let end = if self.csum(&sb) { bs - 12 } else { bs };
        put16(&mut blk, 16, (end - 12) as u64);
        blk[18] = 2;
        blk[19] = 2;
        blk[20] = b'.';
        blk[21] = b'.';
        if self.csum(&sb) {
            put16(&mut blk, bs - 12 + 4, 12);
            blk[bs - 12 + 7] = 0xDE;
        }
        self.set_data(ino, &blk)?;
        let raw = self.get_raw(ino)?;
        let dir = self.inode(ino).ok_or("Lesefehler")?;
        if let Some(&b0) = self.dir_blocks(&dir).first() {
            let mut b = self.block(b0)?;
            self.dir_tail_csum(&sb, ino, &raw, &mut b);
            self.put_block(b0, &b)?;
        }
        self.dir_add(dino, name, ino, 2)?;
        let mut praw = self.get_raw(dino)?;
        let links = le16(&praw, 0x1A) + 1;
        put16(&mut praw, 0x1A, links);
        self.put_inode(dino, &mut praw)
    }

    pub fn unlink(&self, path: &str) -> WResult<()> {
        let (parent, name) = Self::split(path);
        let (dino, dnode) = self.lookup(parent).ok_or("Verzeichnis fehlt")?;
        // the entry itself, never what a link at the name points to (a link is removed as a link,
        // also when its target is a directory or missing)
        let ino = self.list(&dnode).into_iter().find(|e| e.0 == name).map(|e| e.1).ok_or("nicht gefunden")?;
        let node = self.inode(ino).ok_or("Lesefehler")?;
        let dir = node.is_dir();
        if dir && !self.list(&node).is_empty() {
            return Err("Verzeichnis nicht leer");
        }
        let _ = ino;
        let ino = self.dir_remove(dino, name)?;
        let mut raw = self.get_raw(ino)?;
        let links = le16(&raw, 0x1A).saturating_sub(if dir { 2 } else { 1 });
        put16(&mut raw, 0x1A, links);
        if links == 0 {
            let blocks = self.inode_blocks(&raw);
            put32(&mut raw, 0x14, now_secs());
            put32(&mut raw, 4, 0);
            self.put_inode(ino, &mut raw)?;
            self.free_runs(&blocks)?;
            self.free_inode(ino, dir)?;
        } else {
            self.put_inode(ino, &mut raw)?;
        }
        if dir {
            let mut praw = self.get_raw(dino)?;
            let l = le16(&praw, 0x1A).saturating_sub(1);
            put16(&mut praw, 0x1A, l);
            self.put_inode(dino, &mut praw)?;
        }
        Ok(())
    }

    /// Set permission bits (the file type is kept), owner and group of the inode at `path`, and its
    /// change time. Written through like every change of this driver.
    pub fn set_attr(&self, path: &str, perm: u16, uid: u32, gid: u32) -> WResult<()> {
        let (ino, _) = self.lookup(path).ok_or("not found")?;
        let mut raw = self.get_raw(ino)?;
        let kind = le16(&raw, 0) & 0xF000;
        put16(&mut raw, 0, kind | (perm as u64 & 0o7777));
        put16(&mut raw, 2, uid as u64 & 0xffff);
        put16(&mut raw, 0x78, uid as u64 >> 16);
        put16(&mut raw, 24, gid as u64 & 0xffff);
        put16(&mut raw, 0x7A, gid as u64 >> 16);
        put32(&mut raw, 0x0C, now_secs());
        self.put_inode(ino, &mut raw)
    }

    /// Schreiben möglich? (Prüfsummen-Art bekannt, kein offenes Journal)
    pub fn writable(&self) -> bool {
        let Some(sb) = self.sb() else { return false };
        let ro = le32(&sb, 100);
        let incompat = le32(&sb, 96);
        // gdt_csum ohne metadata_csum (crc16) und Journal mit offener Wiederherstellung: nicht schreiben
        !(ro & 0x10 != 0 && ro & 0x400 == 0) && incompat & 0x4 == 0
    }
}

fn now_secs() -> u64 {
    crate::syscall::epoch_secs()
}
