//! Encrypted data partition (LUKS2, `aes-xts-plain64`, 512-byte sectors).
//!
//! The kernel only decrypts: it never sees the passphrase and does not parse LUKS2 metadata.
//! At start [`crate::fs::mount`] finds `ultar-data` with a LUKS2 header and reports it here
//! ([`found`]); `/data` stays unmounted, the partition is offered read only as
//! `/dev/ultar/part/ultar-data` (the header for userspace) and `/sys/kernel/ultar/crypt` reads
//! `locked`. `ultar-luks unlock` (system/core/ultar-luks) opens the header with the passphrase
//! and writes `open <data offset in bytes> <master key, 128 hex digits>` to that file (root
//! with the raw-device right only). The kernel then registers a mapped block device over the
//! data segment and mounts it as `/data` – only if it holds an ext4 file system, so a wrong
//! key is refused (`EKEYREJECTED`) instead of being mounted.
//!
//! Ciphers: RustCrypto `aes` (software, constant-time fixsliced: the kernel does not save
//! SSE state, `--cfg aes_force_soft` in `.cargo/config.toml`) and `xts-mode`.

use crate::driver::{BlockDev, Result, BLOCK};
use aes::cipher::{generic_array::GenericArray, KeyInit};
use aes::Aes256;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;
use xts_mode::{get_tweak_default, Xts128};

const SECTOR: u64 = 512;
const EACCES: i64 = 13;
const EBUSY: i64 = 16;
const EINVAL: i64 = 22;
const EKEYREJECTED: i64 = 129;
/// The control file.
pub const CONTROL: &str = "/sys/kernel/ultar/crypt";
/// Magic and version of a LUKS2 primary header.
const LUKS2: [u8; 8] = *b"LUKS\xba\xbe\x00\x02";

/// Whether a partition starts with a LUKS2 header (`head`: its first sector).
pub fn is_luks2(head: &[u8]) -> bool {
    head.len() >= 8 && head[..8] == LUKS2
}

/// The data segment of a partition, decrypted: sector `lba` of the mapping is sector
/// `first + lba` of device `inner`, tweak `lba`.
pub struct Mapping {
    inner: usize,
    first: u64,
    sectors: u64,
    xts: Xts128<Aes256>,
}

impl Mapping {
    fn place(&self, lba: u64, len: usize) -> Result<u64> {
        let n = len as u64 / SECTOR;
        if len as u64 % SECTOR != 0 || lba.checked_add(n).is_none_or(|end| end > self.sectors) {
            return Err(String::from("outside the encrypted area"));
        }
        Ok(self.first + lba)
    }

    pub fn read(&self, bl: &mut [Box<dyn BlockDev>], lba: u64, buf: &mut [u8]) -> Result<()> {
        let at = self.place(lba, buf.len())?;
        bl.get_mut(self.inner).ok_or_else(|| String::from("device gone"))?.read(at, buf)?;
        self.xts.decrypt_area(buf, SECTOR as usize, u128::from(lba), get_tweak_default);
        Ok(())
    }

    pub fn write(&self, bl: &mut [Box<dyn BlockDev>], lba: u64, data: &[u8]) -> Result<()> {
        let at = self.place(lba, data.len())?;
        let mut sealed = crate::mem::try_vec(data.len()).ok_or_else(|| String::from("no memory"))?;
        sealed.copy_from_slice(data);
        self.xts.encrypt_area(&mut sealed, SECTOR as usize, u128::from(lba), get_tweak_default);
        bl.get_mut(self.inner).ok_or_else(|| String::from("device gone"))?.write(at, &sealed)
    }

    pub fn flush(&self, bl: &mut [Box<dyn BlockDev>]) -> Result<()> {
        bl.get_mut(self.inner).ok_or_else(|| String::from("device gone"))?.flush()
    }
}

/// The mapped device in the block device list (`ultar-data-crypt`).
struct CryptDev(Arc<Mapping>);

impl BlockDev for CryptDev {
    fn name(&self) -> String {
        String::from("ultar-data-crypt")
    }
    fn sectors(&self) -> u64 {
        self.0.sectors
    }
    fn read(&mut self, _lba: u64, _buf: &mut [u8]) -> Result<()> {
        Err(String::from("mapped device: read through driver::read_dev"))
    }
    fn mapping(&self) -> Option<Arc<Mapping>> {
        Some(self.0.clone())
    }
}

/// Left in the list when a refused mapping is not the last entry any more.
struct Gone;

impl BlockDev for Gone {
    fn name(&self) -> String {
        String::from("ultar-data-refused")
    }
    fn sectors(&self) -> u64 {
        0
    }
    fn read(&mut self, _lba: u64, _buf: &mut [u8]) -> Result<()> {
        Err(String::from("refused mapping"))
    }
}

#[derive(Clone, Copy)]
struct Locked {
    disk: usize,
    first: u64,
    sectors: u64,
}

enum State {
    None,
    Locked(Locked),
    /// A key is being tried (reads as `locked`).
    Opening,
    Open,
}

static STATE: Mutex<State> = Mutex::new(State::None);

/// `fs::mount`: the data partition (`first`, `sectors` on `disk`) is encrypted.
pub fn found(disk: usize, first: u64, sectors: u64) {
    *STATE.lock() = State::Locked(Locked { disk, first, sectors });
    crate::rawpart::add_data(disk, first, sectors);
    crate::kprintln!("[crypt] ultar-data is encrypted (LUKS2): /data waits for the passphrase");
}

/// Contents of the control file.
pub fn status() -> &'static str {
    match *STATE.lock() {
        State::None => "none",
        State::Locked(_) | State::Opening => "locked",
        State::Open => "open",
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn wipe(key: &mut [u8; 64]) {
    for b in key.iter_mut() {
        // SAFETY: a valid reference; volatile so the clearing is not optimised away.
        unsafe { core::ptr::write_volatile(b, 0) };
    }
}

/// `open <offset> <hex key>`: maps the data segment and mounts it as `/data`.
pub fn control(text: &str) -> core::result::Result<(), i64> {
    if crate::cred::current().euid() != 0 {
        return Err(-EACCES);
    }
    let mut words = text.split_ascii_whitespace();
    let (Some("open"), Some(off), Some(hex), None) = (words.next(), words.next(), words.next(), words.next()) else {
        return Err(-EINVAL);
    };
    // one opener at a time: the state says "opening" until the mapping is mounted or refused
    let l = {
        let mut s = STATE.lock();
        let State::Locked(l) = *s else { return Err(-EBUSY) };
        *s = State::Opening;
        l
    };
    // the mapping, the device entry and the mount outlive the caller: kernel memory
    let r = crate::heap::kernel(|| open(l, off, hex));
    *STATE.lock() = if r.is_ok() { State::Open } else { State::Locked(l) };
    r
}

fn open(l: Locked, off: &str, hex: &str) -> core::result::Result<(), i64> {
    let off: u64 = off.parse().map_err(|_| -EINVAL)?;
    if off % SECTOR != 0 || off / SECTOR >= l.sectors || hex.len() != 128 {
        return Err(-EINVAL);
    }
    let mut key = [0u8; 64];
    for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
        match (nibble(pair[0]), nibble(pair[1])) {
            (Some(a), Some(b)) => key[i] = a << 4 | b,
            _ => {
                wipe(&mut key);
                return Err(-EINVAL);
            }
        }
    }
    let xts = Xts128::new(Aes256::new(GenericArray::from_slice(&key[..32])), Aes256::new(GenericArray::from_slice(&key[32..])));
    wipe(&mut key);
    let map = Arc::new(Mapping { inner: l.disk, first: l.first + off / SECTOR, sectors: l.sectors - off / SECTOR, xts });
    let index = {
        let mut bl = BLOCK.lock();
        if bl.get(l.disk).is_none_or(|d| d.sector_size() != SECTOR as usize) {
            return Err(-EINVAL);
        }
        bl.push(Box::new(CryptDev(map)));
        bl.len() - 1
    };
    let Some(fs) = crate::ext4::Ext4::probe(index, 0) else {
        let mut bl = BLOCK.lock();
        if bl.len() == index + 1 {
            bl.pop();
        } else {
            bl[index] = Box::new(Gone);
        }
        crate::kprintln!("[crypt] key refused: no ext4 file system behind it");
        return Err(-EKEYREJECTED);
    };
    crate::rawpart::remove_data();
    crate::fs::attach_data(fs);
    Ok(())
}
