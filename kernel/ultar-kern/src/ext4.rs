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
}

pub struct Inode {
    pub mode: u16,
    pub size: u64,
    pub flags: u32,
    block: [u8; 60],
    pub mtime: u32,
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
    let mut b = vec![0u8; bytes.div_ceil(512) * 512];
    let mut bl = crate::driver::BLOCK.lock();
    bl.get_mut(disk)?.read(lba, &mut b).ok()?;
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
        Some(Ext4 { disk, part, bs, ipg: le32(&sb, 40), isize, dsize, gdt: first + 1 })
    }

    fn read_blocks(&self, b: u64, n: u64) -> Option<Vec<u8>> {
        rd(self.disk, self.part + b * self.bs / 512, (n * self.bs) as usize)
    }

    pub fn inode(&self, ino: u64) -> Option<Inode> {
        let g = (ino - 1) / self.ipg;
        let idx = (ino - 1) % self.ipg;
        let doff = g * self.dsize;
        let d = self.read_blocks(self.gdt + doff / self.bs, 1)?;
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
        Some(Inode { mode: le16(i, 0) as u16, size: le32(i, 4) | le32(i, 108) << 32, flags: le32(i, 32) as u32, block, mtime: le32(i, 16) as u32 })
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
        let mut out = vec![0u8; size];
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
    bl.get_mut(disk)?.write(lba, data).ok()
}

pub type WResult<T> = core::result::Result<T, &'static str>;

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
        wr(self.disk, self.part + b * self.bs / 512, data).ok_or("Schreibfehler")
    }
    fn groups(&self, sb: &[u8]) -> u64 {
        let blocks = le32(sb, 4) | if le32(sb, 96) & 0x80 != 0 { le32(sb, 0x150) << 32 } else { 0 };
        (blocks - le32(sb, 20)).div_ceil(le32(sb, 32))
    }
    /// Gruppendeskriptor lesen: (Blocknummer, Versatz, Block)
    fn gd(&self, g: u64) -> WResult<(u64, usize, Vec<u8>)> {
        let off = g * self.dsize;
        let b = self.gdt + off / self.bs;
        Ok((b, (off % self.bs) as usize, self.block(b)?))
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
            if le16(&dblk, o + 18) & 2 != 0 {
                continue; // BLOCK_UNINIT
            }
            let free = Self::gd_field(&dblk, o, 12, 0x2C, wide, 16);
            if free == 0 {
                continue;
            }
            let bmb = Self::gd_field(&dblk, o, 0, 0x20, wide, 32);
            let mut bm = self.block(bmb)?;
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
            // zurückgeben, was schon belegt wurde
            let _ = self.free_runs(&runs);
            return Err("Datenträger voll");
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
            if flags & 1 != 0 {
                continue; // INODE_UNINIT
            }
            let free = Self::gd_field(&dblk, o, 14, 0x2E, wide, 16);
            if free == 0 {
                continue;
            }
            let bmb = Self::gd_field(&dblk, o, 4, 0x24, wide, 32);
            let mut bm = self.block(bmb)?;
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

    /// Inhalt eines Inodes vollständig ersetzen
    fn set_data(&self, ino: u64, data: &[u8]) -> WResult<()> {
        let mut raw = self.get_raw(ino)?;
        if le32(&raw, 32) & 0x1000_0000 != 0 {
            return Err("Inline-Daten nicht unterstützt");
        }
        let old = self.inode_blocks(&raw);
        let nb = (data.len() as u64).div_ceil(self.bs);
        let runs = if nb > 0 { self.alloc_blocks(nb)? } else { Vec::new() };
        if runs.len() > 4 {
            let _ = self.free_runs(&runs);
            return Err("Datei zu zerstückelt");
        }
        let mut logical = 0u64;
        for &(start, len) in &runs {
            let s = (logical * self.bs) as usize;
            let e = ((logical + len) * self.bs) as usize;
            let mut chunk = data[s.min(data.len())..e.min(data.len())].to_vec();
            chunk.resize((len * self.bs) as usize, 0);
            self.put_block_n(start, &chunk)?;
            logical += len;
        }
        // Extent-Kopf im Inode
        let mut ib = [0u8; 60];
        put16(&mut ib, 0, 0xF30A);
        put16(&mut ib, 2, runs.len() as u64);
        put16(&mut ib, 4, 4);
        let mut l = 0u64;
        for (k, &(start, len)) in runs.iter().enumerate() {
            let o = 12 + k * 12;
            put32(&mut ib, o, l);
            put16(&mut ib, o + 4, len);
            put16(&mut ib, o + 6, start >> 32);
            put32(&mut ib, o + 8, start);
            l += len;
        }
        raw[40..100].copy_from_slice(&ib);
        let flags = (le32(&raw, 32) | 0x80000) & !0x1000;
        put32(&mut raw, 32, flags);
        put32(&mut raw, 4, data.len() as u64);
        put32(&mut raw, 108, (data.len() as u64) >> 32);
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
        let (dino, _) = self.lookup(parent).ok_or("Verzeichnis fehlt")?;
        let (ino, node) = self.lookup(path).ok_or("nicht gefunden")?;
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
