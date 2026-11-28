//! The superblock values the growth needs, the layouts it accepts, and the checksums.

use crate::{crc32c, le16, le32, put16};

/// Superblock offsets (inside the 1024-byte superblock).
pub mod sb {
    pub const INODES: usize = 0x00;
    pub const BLOCKS_LO: usize = 0x04;
    pub const R_BLOCKS_LO: usize = 0x08;
    pub const FREE_BLOCKS_LO: usize = 0x0C;
    pub const FREE_INODES: usize = 0x10;
    pub const FIRST_DATA: usize = 0x14;
    pub const BLOCKS_PER_GROUP: usize = 0x20;
    pub const CLUSTERS_PER_GROUP: usize = 0x24;
    pub const INODES_PER_GROUP: usize = 0x28;
    pub const GROUP_NR: usize = 0x5A;
    pub const RESERVED_GDT: usize = 0xCE;
    pub const BLOCKS_HI: usize = 0x150;
    pub const R_BLOCKS_HI: usize = 0x154;
    pub const FREE_BLOCKS_HI: usize = 0x158;
    pub const CHECKSUM: usize = 0x3FC;
}

/// Group descriptor offsets and flags.
pub mod gd {
    pub const BLOCK_BITMAP: (usize, usize) = (0x00, 0x20);
    pub const INODE_BITMAP: (usize, usize) = (0x04, 0x24);
    pub const INODE_TABLE: (usize, usize) = (0x08, 0x28);
    pub const FREE_BLOCKS: (usize, usize) = (0x0C, 0x2C);
    pub const FREE_INODES: (usize, usize) = (0x0E, 0x2E);
    pub const FLAGS: usize = 0x12;
    pub const BLOCK_BITMAP_CSUM: (usize, usize) = (0x18, 0x38);
    pub const ITABLE_UNUSED: (usize, usize) = (0x1C, 0x32);
    pub const CHECKSUM: usize = 0x1E;
    pub const INODE_UNINIT: u64 = 1;
    pub const BLOCK_UNINIT: u64 = 2;
}

const COMPAT_RESIZE_INODE: u64 = 0x10;
const COMPAT_SPARSE_SUPER2: u64 = 0x200;
/// filetype, extents, 64bit, flex_bg, ea_inode, csum_seed, largedir, inline_data, encrypt, casefold.
const INCOMPAT_KNOWN: u64 =
    0x2 | 0x40 | 0x80 | 0x200 | 0x400 | 0x2000 | 0x4000 | 0x8000 | 0x1_0000 | 0x2_0000;
const INCOMPAT_64BIT: u64 = 0x80;
const INCOMPAT_CSUM_SEED: u64 = 0x2000;
/// sparse_super, large_file, huge_file, gdt_csum, dir_nlink, extra_isize, quota, metadata_csum,
/// readonly, project, verity.
const RO_KNOWN: u64 =
    0x1 | 0x2 | 0x8 | 0x10 | 0x20 | 0x40 | 0x100 | 0x400 | 0x1000 | 0x2000 | 0x8000;
const RO_SPARSE: u64 = 0x1;
const RO_METADATA_CSUM: u64 = 0x400;

/// Checks state and features; the incompatible feature bits when the layout is grown.
fn features(sb: &[u8]) -> Result<u64, &'static str> {
    if le16(sb, 0x38) != 0xEF53 {
        return Err("no ext4");
    }
    if le16(sb, 0x3A) & 3 != 1 {
        return Err("not cleanly unmounted or has errors");
    }
    let (compat, incompat, ro) = (le32(sb, 0x5C), le32(sb, 0x60), le32(sb, 0x64));
    if compat & COMPAT_RESIZE_INODE == 0 || compat & COMPAT_SPARSE_SUPER2 != 0 {
        return Err("no resize inode");
    }
    if incompat & !INCOMPAT_KNOWN != 0 {
        return Err("incompatible feature (meta_bg, journal recovery, mmp ...)");
    }
    if ro & !RO_KNOWN != 0 || ro & RO_SPARSE == 0 || ro & RO_METADATA_CSUM == 0 {
        return Err("needs sparse_super and metadata_csum, no bigalloc");
    }
    Ok(incompat)
}

/// The file system as the growth sees it.
pub struct Fs {
    pub sb: Vec<u8>,
    pub bs: u64,
    pub first: u64,
    pub bpg: u64,
    pub ipg: u64,
    pub isize: u64,
    pub dsize: u64,
    pub blocks: u64,
    pub groups: u64,
    pub gdt: u64,
    pub reserved: u64,
    pub seed: u32,
    pub wide: bool,
}

impl Fs {
    /// Checks the superblock; `Err` names why this layout is not grown.
    pub fn parse(sb: Vec<u8>) -> Result<Fs, &'static str> {
        let incompat = features(&sb)?;
        let wide = incompat & INCOMPAT_64BIT != 0;
        let bs = 1024u64 << le32(&sb, 0x18);
        let dsize = if wide { le16(&sb, 0xFE) } else { 32 };
        let bpg = le32(&sb, sb::BLOCKS_PER_GROUP);
        if bpg != le32(&sb, sb::CLUSTERS_PER_GROUP)
            || bpg != bs * 8
            || !(32..=1024).contains(&dsize)
        {
            return Err("unusual group layout");
        }
        let blocks = le32(&sb, sb::BLOCKS_LO)
            | if wide {
                le32(&sb, sb::BLOCKS_HI) << 32
            } else {
                0
            };
        let first = le32(&sb, sb::FIRST_DATA);
        let groups = (blocks - first).div_ceil(bpg);
        let seed = if incompat & INCOMPAT_CSUM_SEED != 0 {
            le32(&sb, 0x270) as u32
        } else {
            crc32c(!0, &sb[0x68..0x78])
        };
        let isize = if le32(&sb, 0x4C) >= 1 {
            le16(&sb, 0x58)
        } else {
            128
        };
        Ok(Fs {
            gdt: (groups * dsize).div_ceil(bs),
            reserved: le16(&sb, sb::RESERVED_GDT),
            ipg: le32(&sb, sb::INODES_PER_GROUP),
            sb,
            bs,
            first,
            bpg,
            isize,
            dsize,
            blocks,
            groups,
            seed,
            wide,
        })
    }

    /// Whether group `g` holds a superblock backup (sparse_super: 0, 1, powers of 3, 5, 7).
    pub fn has_super(g: u64) -> bool {
        if g <= 1 {
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

    /// First block of group `g`.
    pub fn start(&self, g: u64) -> u64 {
        self.first + g * self.bpg
    }

    /// Blocks of one inode table.
    pub fn itable_blocks(&self) -> u64 {
        (self.ipg * self.isize).div_ceil(self.bs)
    }

    /// Descriptors per block.
    pub fn per_block(&self) -> u64 {
        self.bs / self.dsize
    }

    /// A descriptor field (`lo`, `hi` offsets; 16 or 32 bits each half).
    pub fn get(&self, d: &[u8], f: (usize, usize), bits: u32) -> u64 {
        let half = |o| if bits == 16 { le16(d, o) } else { le32(d, o) };
        half(f.0)
            | if self.dsize >= 64 {
                half(f.1) << bits
            } else {
                0
            }
    }

    /// Sets a descriptor field.
    pub fn set(&self, d: &mut [u8], f: (usize, usize), bits: u32, v: u64) {
        let mut half = |o, x: u64| {
            if bits == 16 {
                put16(d, o, x & 0xffff)
            } else {
                crate::put32(d, o, x & 0xffff_ffff)
            }
        };
        half(f.0, v);
        if self.dsize >= 64 {
            half(f.1, v >> bits);
        }
    }

    /// Stores the checksum of descriptor `d` of group `g`.
    pub fn desc_csum(&self, g: u64, d: &mut [u8]) {
        put16(d, gd::CHECKSUM, 0);
        let c = crc32c(crc32c(self.seed, &(g as u32).to_le_bytes()), d);
        put16(d, gd::CHECKSUM, u64::from(c & 0xffff));
    }

    /// Checksum of a bitmap of `bytes` bytes (lower and upper half go to the descriptor).
    pub fn bitmap_csum(&self, bitmap: &[u8], bytes: usize) -> u64 {
        u64::from(crc32c(self.seed, &bitmap[..bytes]))
    }

    /// Stores the checksum of inode `ino` (raw, `isize` bytes).
    pub fn inode_csum(&self, ino: u64, raw: &mut [u8]) {
        let hi = raw.len() > 128 && le16(raw, 0x80) >= 4;
        put16(raw, 0x7C, 0);
        if hi {
            put16(raw, 0x82, 0);
        }
        let c = crc32c(self.seed, &(ino as u32).to_le_bytes());
        let c = crc32c(c, &raw[0x64..0x68]);
        let c = crc32c(c, raw);
        put16(raw, 0x7C, u64::from(c & 0xffff));
        if hi {
            put16(raw, 0x82, u64::from(c >> 16));
        }
    }

    /// Stores the superblock checksum.
    pub fn super_csum(sb: &mut [u8]) {
        let c = crc32c(!0, &sb[..sb::CHECKSUM]);
        crate::put32(sb, sb::CHECKSUM, u64::from(c));
    }
}
