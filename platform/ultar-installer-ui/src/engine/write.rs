//! Step 15: writing the A/B layout (the only code that writes to a disk).
//!
//! Order (an interrupted installation must never look valid): whole disk – old partition
//! table zeroed first; beside / manual – the table on the disk is checked against the one the
//! user confirmed and stays valid until the end (other systems survive an interruption);
//! partitions copied and read back; empty slots zeroed; boot control + answers; backup GPT,
//! then primary GPT (kept entries bit for bit); flush; primary header read back.

use super::answers::{self, Answers};
use super::device::{BlockDevice, FileDevice};
use super::{existing, read_gpt, same_device, EngineError, EraseConfirmation, Plan};
use ultar_slots::gpt::Header;
use ultar_slots::install::{
    self, Fill, Target, BACK_BYTES, ENTRY_COUNT, ENTRY_SIZE, FRONT_BYTES, MIB, PARTS,
};
use ultar_slots::layout::BOOTCTL_INSTALL_BYTES;

/// Bytes copied per read/write.
pub const CHUNK: usize = 1 << 20;

/// Progress: bytes done of bytes in all (copy and verify count once each).
pub trait Progress {
    /// Called after every chunk.
    fn step(&mut self, done: u64, total: u64);
}

impl<F: FnMut(u64, u64)> Progress for F {
    fn step(&mut self, done: u64, total: u64) {
        self(done, total)
    }
}

fn random_guids() -> Result<([[u8; 16]; PARTS], [u8; 16]), EngineError> {
    let mut bytes = [0u8; 16 * (PARTS + 1)];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| EngineError::Io("random", std::io::ErrorKind::Other))?;
    let mut guids = [[0u8; 16]; PARTS];
    for (i, g) in guids.iter_mut().enumerate() {
        g.copy_from_slice(&bytes[i * 16..(i + 1) * 16]);
        g[7] = (g[7] & 0x0f) | 0x40;
        g[8] = (g[8] & 0x3f) | 0x80;
    }
    let mut disk = [0u8; 16];
    disk.copy_from_slice(&bytes[PARTS * 16..]);
    disk[7] = (disk[7] & 0x0f) | 0x40;
    disk[8] = (disk[8] & 0x3f) | 0x80;
    Ok((guids, disk))
}

fn write_verified(
    dev: &mut dyn BlockDevice,
    offset: u64,
    data: &[u8],
    what: &'static str,
) -> Result<(), EngineError> {
    dev.write_at(offset, data)?;
    let mut back = vec![0u8; data.len()];
    dev.read_at(offset, &mut back)?;
    if back != data {
        return Err(EngineError::Verify(what));
    }
    Ok(())
}

fn copy(
    src: &mut dyn BlockDevice,
    dst: &mut dyn BlockDevice,
    t: &Target,
    range: (u64, u64),
    done: &mut u64,
    total: u64,
    progress: &mut dyn Progress,
) -> Result<(), EngineError> {
    let bytes = (range.1 - range.0 + 1) * 512;
    let mut buf = vec![0u8; CHUNK];
    let mut back = vec![0u8; CHUNK];
    let mut off = 0u64;
    while off < bytes {
        let n = (bytes - off).min(CHUNK as u64) as usize;
        src.read_at(range.0 * 512 + off, &mut buf[..n])?;
        let target = t.first * 512 + off;
        dst.write_at(target, &buf[..n])?;
        dst.read_at(target, &mut back[..n])?;
        if back[..n] != buf[..n] {
            return Err(EngineError::Verify(t.name));
        }
        off += n as u64;
        *done += n as u64;
        progress.step(*done, total);
    }
    Ok(())
}

/// The data partition as LUKS2 volume: header and keyslot written first (the rest of the
/// metadata area zeroed), then the medium's data encrypted behind it, every chunk read back.
#[allow(clippy::too_many_arguments)]
fn copy_encrypted(
    src: &mut dyn BlockDevice,
    dst: &mut dyn BlockDevice,
    t: &Target,
    range: (u64, u64),
    plan: &Plan,
    pass: &crate::secret::Secret,
    done: &mut u64,
    total: u64,
    progress: &mut dyn Progress,
) -> Result<ultar_luks::Sectors, EngineError> {
    let random = |_| EngineError::Io("luks", std::io::ErrorKind::Other);
    // the unlock screen at start speaks the installation's language and knows its keyboard
    let start = ultar_luks::StartInfo {
        lang: plan.answers.get("lang").unwrap_or("en").to_owned(),
        keyboard: plan.answers.get("keyboard").unwrap_or("us").to_owned(),
    };
    let volume = ultar_luks::format_with(pass.expose().as_bytes(), &plan.luks, Some(&start))
        .map_err(random)?;
    let mut meta = volume.metadata;
    meta.resize(ultar_luks::DATA_OFFSET as usize, 0);
    write_verified(dst, t.first * 512, &meta, t.name)?;
    let sectors = ultar_luks::Sectors::from_key(&volume.key);
    let base = t.first * 512 + ultar_luks::DATA_OFFSET;
    let bytes = (range.1 - range.0 + 1) * 512;
    let mut buf = zeroize::Zeroizing::new(vec![0u8; CHUNK]);
    let mut back = vec![0u8; CHUNK];
    let mut off = 0u64;
    while off < bytes {
        let n = (bytes - off).min(CHUNK as u64) as usize;
        src.read_at(range.0 * 512 + off, &mut buf[..n])?;
        sectors.encrypt(off / 512, &mut buf[..n]);
        dst.write_at(base + off, &buf[..n])?;
        dst.read_at(base + off, &mut back[..n])?;
        if back[..n] != buf[..n] {
            return Err(EngineError::Verify(t.name));
        }
        off += n as u64;
        *done += n as u64;
        progress.step(*done, total);
    }
    Ok(sectors)
}

fn bootctl(answers: &Answers) -> Vec<u8> {
    let mut img = vec![0u8; BOOTCTL_INSTALL_BYTES as usize];
    install::bootctl_image(&mut img);
    let record = answers.encode();
    let end = answers::OFFSET + record.len();
    if end <= img.len() {
        img[answers::OFFSET..end].copy_from_slice(&record);
    }
    img
}

type KeptTable = (Box<[[u8; ENTRY_SIZE]; ENTRY_COUNT]>, [u8; 16]);

/// The entries to keep, checked against the table on the disk now (same entries, same
/// places), and the disk GUID; refuses when the disk changed since the user confirmed.
fn kept_entries(
    dst: &mut dyn BlockDevice,
    plan: &Plan,
    keep: &[super::Existing],
) -> Result<KeptTable, EngineError> {
    let now = read_gpt(dst).map(|(h, e)| (existing(&h, &e), h.disk_guid));
    let (table, disk) = match now {
        Some(t) => t,
        None if keep.is_empty() => (Vec::new(), random_guids()?.1),
        None => return Err(EngineError::TargetChanged),
    };
    let mut entries = Box::new([[0u8; ENTRY_SIZE]; ENTRY_COUNT]);
    for k in keep {
        if !table.contains(k) || k.index >= ENTRY_COUNT {
            return Err(EngineError::TargetChanged);
        }
        entries[k.index] = k.entry;
    }
    let planned: Vec<usize> = keep.iter().chain(&plan.removed).map(|e| e.index).collect();
    if table.iter().any(|e| !planned.contains(&e.index)) {
        return Err(EngineError::TargetChanged);
    }
    Ok((entries, disk))
}

/// Copies, zeroes and fills the planned partitions, each read back.
fn write_parts(
    plan: &Plan,
    src: &mut dyn BlockDevice,
    dst: &mut dyn BlockDevice,
    progress: &mut dyn Progress,
) -> Result<(), EngineError> {
    let total = plan.copy_bytes().max(1);
    let mut done = 0u64;
    for t in install::parts(&plan.layout) {
        match (t.fill, &plan.encrypt) {
            (Fill::Copy(range), Some(pass)) if t.name == ultar_slots::layout::DATA_PART => {
                let xts =
                    copy_encrypted(src, dst, t, range, plan, pass, &mut done, total, progress)?;
                let (base, bytes) = (
                    t.first * 512 + ultar_luks::DATA_OFFSET,
                    t.sectors * 512 - ultar_luks::DATA_OFFSET,
                );
                super::grow::data(dst, base, bytes, Some(&xts))?;
            }
            (Fill::Copy(range), _) => {
                copy(src, dst, t, range, &mut done, total, progress)?;
                if t.name == ultar_slots::layout::DATA_PART {
                    super::grow::data(dst, t.first * 512, t.sectors * 512, None)?;
                }
            }
            (Fill::Zero, _) => {
                let n = (t.sectors.min(MIB) * 512) as usize;
                write_verified(dst, t.first * 512, &vec![0u8; n], t.name)?;
            }
            (Fill::BootCtl, _) => {
                write_verified(dst, t.first * 512, &bootctl(&plan.answers), t.name)?
            }
        }
    }
    Ok(())
}

/// Writes `plan` from `src` onto `dst`. Needs the user's [`EraseConfirmation`] for exactly
/// this target and size.
pub fn execute_on(
    plan: &Plan,
    confirm: &EraseConfirmation,
    src: &mut dyn BlockDevice,
    dst: &mut dyn BlockDevice,
    progress: &mut dyn Progress,
) -> Result<(), EngineError> {
    if confirm.target != plan.target
        || confirm.bytes != plan.target_bytes
        || dst.len() != plan.target_bytes
    {
        return Err(EngineError::TargetChanged);
    }
    let sectors = dst.len() / 512;
    if sectors < install::needed(&plan.layout) {
        return Err(EngineError::TooSmall(
            install::needed(&plan.layout) * 512,
            dst.len(),
        ));
    }
    // Keeping partitions: the table on the disk must still be the one the user confirmed;
    // nothing outside the new partitions is written, the old table stays valid until the
    // new one is written last (an interrupted installation leaves the old systems intact).
    let kept = match &plan.keep {
        Some(keep) => Some(kept_entries(dst, plan, keep)?),
        None => {
            write_verified(dst, 0, &[0u8; FRONT_BYTES], "GPT")?;
            write_verified(dst, (sectors - 33) * 512, &[0u8; BACK_BYTES], "GPT")?;
            None
        }
    };
    write_parts(plan, src, dst, progress)?;
    let (guids, disk) = random_guids()?;
    let (mut front, mut back) = (Box::new([0u8; FRONT_BYTES]), Box::new([0u8; BACK_BYTES]));
    let back_lba = match &kept {
        Some((entries, old_disk)) => install::gpt_keeping(
            &plan.layout,
            entries,
            sectors,
            &guids,
            *old_disk,
            &mut front,
            &mut back,
        ),
        None => install::gpt(&plan.layout, sectors, &guids, disk, &mut front, &mut back),
    }
    .ok_or(EngineError::Gpt)?;
    write_verified(dst, back_lba * 512, &back[..], "GPT")?;
    write_verified(dst, 0, &front[..], "GPT")?;
    dst.flush()?;
    let total = plan.copy_bytes().max(1);
    let mut header = [0u8; 512];
    dst.read_at(512, &mut header)?;
    Header::parse(&header, 1).map_err(|_| EngineError::Gpt)?;
    progress.step(total, total);
    Ok(())
}

/// Opens medium (read) and target (write) and runs [`execute_on`].
pub fn execute(
    plan: &Plan,
    confirm: &EraseConfirmation,
    progress: &mut dyn Progress,
) -> Result<(), EngineError> {
    if same_device(&plan.source, &plan.target) {
        return Err(EngineError::TargetIsSource);
    }
    let mut src = FileDevice::open_read(&plan.source)?;
    let mut dst = FileDevice::open_write(&plan.target)?;
    execute_on(plan, confirm, &mut src, &mut dst, progress)
}
