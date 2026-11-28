//! The growth itself: plan, last group, new groups, resize inode, superblock and backups.

use crate::layout::{gd, sb, Fs};
use crate::{le16, le32, put16, put32, Disk, Error, Outcome};

/// Smallest new group worth adding (blocks of data beside its metadata).
const MIN_DATA: u64 = 64;
/// The resize inode and its double-indirect slot.
const RESIZE_INO: u64 = 7;
const DIND: usize = 0x28 + 13 * 4;

struct Plan {
    blocks: u64,
    groups: u64,
    gdt: u64,
    reserved: u64,
}

impl Plan {
    fn meta(&self, g: u64) -> u64 {
        if Fs::has_super(g) {
            1 + self.gdt + self.reserved
        } else {
            0
        }
    }
}

fn plan(fs: &Fs, bytes: u64) -> Option<Plan> {
    let mut target = bytes / fs.bs;
    if !fs.wide {
        target = target.min(u64::from(u32::MAX));
    }
    let max_groups = ((fs.gdt + fs.reserved) * fs.per_block()).min(u64::from(u32::MAX) / fs.ipg);
    let mut groups = target
        .checked_sub(fs.first)?
        .div_ceil(fs.bpg)
        .min(max_groups);
    loop {
        let blocks = target.min(fs.start(groups));
        let gdt = groups.div_ceil(fs.per_block());
        let p = Plan {
            blocks,
            groups,
            gdt,
            reserved: fs.reserved + fs.gdt - gdt,
        };
        let last = groups - 1;
        let room = blocks - fs.start(last);
        if last >= fs.groups && room < p.meta(last) + 2 + fs.itable_blocks() + MIN_DATA {
            groups -= 1;
            continue;
        }
        return (blocks > fs.blocks).then_some(p);
    }
}

/// Reads block `b` of the file system.
fn block(disk: &mut dyn Disk, fs: &Fs, b: u64) -> Result<Vec<u8>, Error> {
    let mut v = vec![0u8; fs.bs as usize];
    disk.read_at(b * fs.bs, &mut v)?;
    Ok(v)
}

/// Grows the ext4 on `disk` to `bytes` (at most); see the crate documentation.
pub fn grow(disk: &mut dyn Disk, bytes: u64) -> Result<Outcome, Error> {
    let mut raw = vec![0u8; 1024];
    disk.read_at(1024, &mut raw)?;
    let fs = match Fs::parse(raw) {
        Ok(fs) => fs,
        Err(why) => return Ok(Outcome::Unchanged(why)),
    };
    let Some(p) = plan(&fs, bytes) else {
        return Ok(Outcome::Unchanged("no room to grow"));
    };
    let backups = (1..p.groups).filter(|&g| Fs::has_super(g)).count() as u64;
    if backups > fs.bs / 4 {
        return Ok(Outcome::Unchanged("too many backup groups"));
    }
    let mut gdt = vec![0u8; (p.gdt * fs.bs) as usize];
    disk.read_at(
        (fs.first + 1) * fs.bs,
        &mut gdt[..(fs.gdt * fs.bs) as usize],
    )?;
    let resize = resize_inode(disk, &fs, &gdt)?;
    if le32(&resize.1[resize.2..], DIND) == 0 {
        return Ok(Outcome::Unchanged("resize inode without blocks"));
    }
    let mut free = fill_last(disk, &fs, &p, &mut gdt)?;
    let (added, last) = add_groups(&fs, &p, &mut gdt);
    free += added;
    if let Some((at, bm)) = last {
        disk.write_at(at * fs.bs, &bm)?;
    }
    write_resize_inode(disk, &fs, &p, backups, resize)?;
    let sb = new_super(&fs, &p, free);
    write_everywhere(disk, &fs, &p, &sb, &gdt)?;
    Ok(Outcome::Grown {
        from: fs.blocks,
        to: p.blocks,
    })
}

fn desc<'a>(fs: &Fs, gdt: &'a mut [u8], g: u64) -> &'a mut [u8] {
    let o = (g * fs.dsize) as usize;
    &mut gdt[o..o + fs.dsize as usize]
}

/// The last group of the old file system takes the blocks up to its end.
fn fill_last(disk: &mut dyn Disk, fs: &Fs, p: &Plan, gdt: &mut [u8]) -> Result<u64, Error> {
    let g = fs.groups - 1;
    let start = fs.start(g);
    let (old, new) = (fs.blocks - start, fs.bpg.min(p.blocks - start));
    if new <= old {
        return Ok(0);
    }
    let d = desc(fs, gdt, g);
    if le16(d, gd::FLAGS) & gd::BLOCK_UNINIT == 0 {
        let at = fs.get(d, gd::BLOCK_BITMAP, 32);
        let mut bm = block(disk, fs, at)?;
        for i in old..new {
            bm[(i / 8) as usize] &= !(1 << (i % 8));
        }
        disk.write_at(at * fs.bs, &bm)?;
        let c = fs.bitmap_csum(&bm, (fs.bpg / 8) as usize);
        fs.set(d, gd::BLOCK_BITMAP_CSUM, 16, c);
    }
    let f = fs.get(d, gd::FREE_BLOCKS, 16) + new - old;
    fs.set(d, gd::FREE_BLOCKS, 16, f);
    fs.desc_csum(g, d);
    Ok(new - old)
}

/// Descriptors of the new groups: bitmaps and inode table at the start of each group (after
/// its superblock backup and descriptor copies), nothing initialised yet.
fn add_groups(fs: &Fs, p: &Plan, gdt: &mut [u8]) -> (u64, Option<(u64, Vec<u8>)>) {
    let mut free = 0;
    let mut bitmap = None;
    for g in fs.groups..p.groups {
        let start = fs.start(g);
        let size = fs.bpg.min(p.blocks - start);
        let meta = p.meta(g);
        let d = desc(fs, gdt, g);
        d.fill(0);
        fs.set(d, gd::BLOCK_BITMAP, 32, start + meta);
        fs.set(d, gd::INODE_BITMAP, 32, start + meta + 1);
        fs.set(d, gd::INODE_TABLE, 32, start + meta + 2);
        let f = size - meta - 2 - fs.itable_blocks();
        fs.set(d, gd::FREE_BLOCKS, 16, f);
        fs.set(d, gd::FREE_INODES, 16, fs.ipg);
        fs.set(d, gd::ITABLE_UNUSED, 16, fs.ipg);
        put16(d, gd::FLAGS, gd::INODE_UNINIT | gd::BLOCK_UNINIT);
        if g == p.groups - 1 {
            // the last group needs a real block bitmap (e2fsck: "last group block bitmap
            // uninitialized"); its padding past the group's end is set
            let bm = last_bitmap(fs, meta, size);
            let c = fs.bitmap_csum(&bm, (fs.bpg / 8) as usize);
            fs.set(d, gd::BLOCK_BITMAP_CSUM, 16, c);
            put16(d, gd::FLAGS, gd::INODE_UNINIT);
            bitmap = Some((start + meta, bm));
        }
        fs.desc_csum(g, d);
        free += f;
    }
    (free, bitmap)
}

/// Block bitmap of a new group: its metadata in use, the bits past its end set.
fn last_bitmap(fs: &Fs, meta: u64, size: u64) -> Vec<u8> {
    let mut bm = vec![0u8; fs.bs as usize];
    let used = meta + 2 + fs.itable_blocks();
    for i in (0..used).chain(size..fs.bs * 8) {
        bm[(i / 8) as usize] |= 1 << (i % 8);
    }
    bm
}

/// The resize inode: (block number, the block, offset of the inode in it).
fn resize_inode(disk: &mut dyn Disk, fs: &Fs, gdt: &[u8]) -> Result<(u64, Vec<u8>, usize), Error> {
    let table = fs.get(&gdt[..fs.dsize as usize], gd::INODE_TABLE, 32);
    let at = (RESIZE_INO - 1) * fs.isize;
    let b = table + at / fs.bs;
    Ok((b, block(disk, fs, b)?, (at % fs.bs) as usize))
}

/// Rebuilds the resize inode for the reserved descriptor blocks that remain: the
/// double-indirect block lists them, each lists its copies in the backup groups.
fn write_resize_inode(
    disk: &mut dyn Disk,
    fs: &Fs,
    p: &Plan,
    backups: u64,
    inode: (u64, Vec<u8>, usize),
) -> Result<(), Error> {
    let (at, mut blk, off) = inode;
    let raw = &mut blk[off..off + fs.isize as usize];
    let dind_at = le32(raw, DIND);
    let per = fs.bs / 4;
    let mut dind = vec![0u8; fs.bs as usize];
    for j in 0..p.reserved {
        let primary = fs.first + 1 + p.gdt + j;
        put32(&mut dind, (((p.gdt + j) % per) * 4) as usize, primary);
        let mut ind = vec![0u8; fs.bs as usize];
        for (k, g) in (1..p.groups).filter(|&g| Fs::has_super(g)).enumerate() {
            put32(&mut ind, k * 4, primary + g * fs.bpg);
        }
        disk.write_at(primary * fs.bs, &ind)?;
    }
    disk.write_at(dind_at * fs.bs, &dind)?;
    let sectors = (1 + p.reserved * (1 + backups)) * (fs.bs / 512);
    put32(raw, 0x1C, sectors);
    put16(raw, 0x74, sectors >> 32);
    fs.inode_csum(RESIZE_INO, raw);
    disk.write_at(at * fs.bs, &blk)?;
    Ok(())
}

fn new_super(fs: &Fs, p: &Plan, free: u64) -> Vec<u8> {
    let mut s = fs.sb.clone();
    let get64 = |s: &[u8], lo, hi| le32(s, lo) | if fs.wide { le32(s, hi) << 32 } else { 0 };
    let put64 = |s: &mut [u8], lo, hi, v: u64| {
        put32(s, lo, v);
        if fs.wide {
            put32(s, hi, v >> 32);
        }
    };
    let reserved_blocks = (u128::from(get64(&s, sb::R_BLOCKS_LO, sb::R_BLOCKS_HI))
        * u128::from(p.blocks)
        / u128::from(fs.blocks)) as u64;
    put64(&mut s, sb::BLOCKS_LO, sb::BLOCKS_HI, p.blocks);
    put64(&mut s, sb::R_BLOCKS_LO, sb::R_BLOCKS_HI, reserved_blocks);
    let free_blocks = get64(&s, sb::FREE_BLOCKS_LO, sb::FREE_BLOCKS_HI) + free;
    put64(&mut s, sb::FREE_BLOCKS_LO, sb::FREE_BLOCKS_HI, free_blocks);
    let added = (p.groups - fs.groups) * fs.ipg;
    put32(&mut s, sb::INODES, p.groups * fs.ipg);
    let free_inodes = le32(&s, sb::FREE_INODES) + added;
    put32(&mut s, sb::FREE_INODES, free_inodes);
    put16(&mut s, sb::RESERVED_GDT, p.reserved);
    s
}

/// Superblock and descriptors to every backup group, the primary ones last.
fn write_everywhere(
    disk: &mut dyn Disk,
    fs: &Fs,
    p: &Plan,
    sb: &[u8],
    gdt: &[u8],
) -> Result<(), Error> {
    for g in (1..p.groups).filter(|&g| Fs::has_super(g)) {
        let start = fs.start(g);
        let mut copy = sb.to_vec();
        put16(&mut copy, sb::GROUP_NR, g);
        Fs::super_csum(&mut copy);
        disk.write_at(start * fs.bs, &copy)?;
        disk.write_at((start + 1) * fs.bs, gdt)?;
    }
    disk.write_at((fs.first + 1) * fs.bs, gdt)?;
    let mut primary = sb.to_vec();
    Fs::super_csum(&mut primary);
    disk.write_at(1024, &primary).map_err(Error::from)
}
