//! The interface programs use: the Linux `xe` DRM uAPI, so that Mesa (Vulkan, OpenGL) from the
//! Ubuntu packages runs unchanged (stage G4, decision in `docs/integration/intel-gt.md` §1).
//! This module is the part without hardware: request numbers and the device queries, as bytes
//! in the layout of `include/uapi/drm/xe_drm.h` (MIT). Answers follow Linux' `xe_query.c` (MIT).
//!
//! Stages G4a/G4b: device queries and the requests of DRM itself ([`drm`]). Buffers, address
//! spaces, exec queues and exec follow.

use alloc::vec;
use alloc::vec::Vec;

pub mod drm;
pub mod file;

/// First driver request number of DRM.
pub const DRM_COMMAND_BASE: u32 = 0x40;
/// Driver request: ask about the device.
pub const XE_DEVICE_QUERY: u32 = 0x00;
/// Driver request: create a buffer.
pub const XE_GEM_CREATE: u32 = 0x01;
/// Driver request: where to map a buffer.
pub const XE_GEM_MMAP_OFFSET: u32 = 0x02;
/// Driver request: create an address space.
pub const XE_VM_CREATE: u32 = 0x03;
/// Driver request: destroy an address space.
pub const XE_VM_DESTROY: u32 = 0x04;
/// Driver request: map buffers into an address space.
pub const XE_VM_BIND: u32 = 0x05;
/// Driver request: create an exec queue.
pub const XE_EXEC_QUEUE_CREATE: u32 = 0x06;
/// Driver request: destroy an exec queue.
pub const XE_EXEC_QUEUE_DESTROY: u32 = 0x07;
/// Driver request: read a property of an exec queue.
pub const XE_EXEC_QUEUE_GET_PROPERTY: u32 = 0x08;
/// Driver request: run a batch.
pub const XE_EXEC: u32 = 0x09;
/// Driver request: wait for a value a batch stores.
pub const XE_WAIT_USER_FENCE: u32 = 0x0a;

/// A decoded request number of `ioctl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// Number within DRM (driver requests start at [`DRM_COMMAND_BASE`]).
    pub nr: u32,
    /// Size of the argument structure.
    pub size: u32,
    /// The kernel reads the structure.
    pub write: bool,
    /// The kernel writes the structure back.
    pub read: bool,
}

impl Request {
    /// Decodes a request number; `None` when it is not a DRM request (type `'d'`).
    pub fn decode(req: u32) -> Option<Request> {
        if (req >> 8) & 0xff != b'd' as u32 {
            return None;
        }
        Some(Request {
            nr: req & 0xff,
            size: (req >> 16) & 0x3fff,
            write: req & (1 << 30) != 0,
            read: req & (1 << 31) != 0,
        })
    }

    /// The driver request (0 = device query …), if this is one.
    pub fn driver(&self) -> Option<u32> {
        (DRM_COMMAND_BASE..DRM_COMMAND_BASE + 0x60)
            .contains(&self.nr)
            .then(|| self.nr - DRM_COMMAND_BASE)
    }
}

/// Why a request is refused (Linux error numbers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    /// Bad argument.
    Inval = 22,
    /// No such device (the part asked about is not there).
    NoDev = 19,
    /// The program's address is not its memory.
    Fault = 14,
    /// Not a request for this kind of file.
    NoTty = 25,
    /// No such object (unknown handle or id).
    NoEnt = 2,
    /// Not now: ask again.
    Again = 11,
    /// Out of memory or over a limit.
    NoMem = 12,
    /// Still in use.
    Busy = 16,
    /// The time of a wait is over.
    Time = 62,
    /// Not supported (yet).
    OpNotSupp = 95,
    /// The queue was stopped because a job of it hung.
    Canceled = 125,
}

/// Engine classes as programs name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineClass {
    /// Render.
    Render = 0,
    /// Copy.
    Copy = 1,
    /// Video decode.
    VideoDecode = 2,
    /// Video enhancement.
    VideoEnhance = 3,
    /// Compute.
    Compute = 4,
}

/// One GT of the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gt {
    /// Media GT instead of the main one.
    pub media: bool,
    /// Number of the GT.
    pub id: u16,
    /// Frequency of the GT's time stamp counter in Hz.
    pub reference_clock: u32,
    /// IP version: architecture, release, revision.
    pub ip: (u16, u16, u16),
}

/// What the queries answer from.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Device {
    /// PCI device id.
    pub devid: u16,
    /// PCI revision.
    pub revid: u8,
    /// Engines programs may use: class, instance, GT.
    pub engines: Vec<(EngineClass, u16, u16)>,
    /// The GTs that are in operation.
    pub gts: Vec<Gt>,
    /// Memory the GPU may use, in bytes (system memory; there is no dedicated memory).
    pub memory_bytes: u64,
    /// Memory in use by buffers, in bytes.
    pub memory_used: u64,
    /// The hardware config table as the GuC handed it out.
    pub hwconfig: Vec<u8>,
    /// Version of the GuC's submission interface: branch, major, minor, patch.
    pub guc_submission: (u32, u32, u32, u32),
}

/// Query: engines.
pub const QUERY_ENGINES: u32 = 0;
/// Query: memory regions.
pub const QUERY_MEM_REGIONS: u32 = 1;
/// Query: configuration.
pub const QUERY_CONFIG: u32 = 2;
/// Query: GTs.
pub const QUERY_GT_LIST: u32 = 3;
/// Query: hardware config table.
pub const QUERY_HWCONFIG: u32 = 4;
/// Query: firmware versions.
pub const QUERY_UC_FW_VERSION: u32 = 7;

/// Size of `struct drm_xe_device_query`.
pub const DEVICE_QUERY_SIZE: usize = 40;
/// Bits of a context's address space.
pub const VA_BITS: u64 = 48;

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

impl Device {
    fn engines(&self) -> Vec<u8> {
        // header 8 bytes, then 32 bytes per engine
        let mut b = vec![0u8; 8 + self.engines.len() * 32];
        put32(&mut b, 0, self.engines.len() as u32);
        for (i, (class, instance, gt)) in self.engines.iter().enumerate() {
            let at = 8 + i * 32;
            put16(&mut b, at, *class as u16);
            put16(&mut b, at + 2, *instance);
            put16(&mut b, at + 4, *gt);
        }
        b
    }

    fn mem_regions(&self) -> Vec<u8> {
        // header 8 bytes, then one region of 88 bytes: system memory, instance 0
        let mut b = vec![0u8; 8 + 88];
        put32(&mut b, 0, 1);
        put32(&mut b, 8 + 4, 4096);
        put64(&mut b, 8 + 8, self.memory_bytes);
        put64(&mut b, 8 + 16, self.memory_used);
        b
    }

    fn config(&self) -> Vec<u8> {
        let info = [
            self.devid as u64 | (self.revid as u64) << 16,
            // no optional feature is announced before the kernel has it
            0,
            4096,
            VA_BITS,
            // highest priority a program may ask for: normal
            1,
        ];
        let mut b = vec![0u8; 8 + info.len() * 8];
        put32(&mut b, 0, info.len() as u32);
        for (i, v) in info.iter().enumerate() {
            put64(&mut b, 8 + i * 8, *v);
        }
        b
    }

    fn gt_list(&self) -> Vec<u8> {
        // header 8 bytes, then 96 bytes per GT
        let mut b = vec![0u8; 8 + self.gts.len() * 96];
        put32(&mut b, 0, self.gts.len() as u32);
        for (i, gt) in self.gts.iter().enumerate() {
            let at = 8 + i * 96;
            put16(&mut b, at, gt.media as u16);
            put16(&mut b, at + 4, gt.id);
            put32(&mut b, at + 12, gt.reference_clock);
            // near memory: region 0 (system memory); nothing is far
            put64(&mut b, at + 16, 1);
            put16(&mut b, at + 32, gt.ip.0);
            put16(&mut b, at + 34, gt.ip.1);
            put16(&mut b, at + 36, gt.ip.2);
        }
        b
    }

    fn uc_fw_version(&self, input: &[u8]) -> Result<Vec<u8>, Errno> {
        // 32 bytes; the program names the firmware in the first two and leaves the rest zero
        let mut b = input.get(..32).ok_or(Errno::Inval)?.to_vec();
        if b[2..].iter().any(|x| *x != 0) {
            return Err(Errno::Inval);
        }
        match u16::from_le_bytes([b[0], b[1]]) {
            0 => {
                let (branch, major, minor, patch) = self.guc_submission;
                put32(&mut b, 4, branch);
                put32(&mut b, 8, major);
                put32(&mut b, 12, minor);
                put32(&mut b, 16, patch);
                Ok(b)
            }
            // the HuC is not loaded
            1 => Err(Errno::NoDev),
            _ => Err(Errno::Inval),
        }
    }

    /// Answers device query `query`. `size` is what the program says its buffer holds and
    /// `input` what is in it (only read for queries that take arguments). A program first asks
    /// with size 0 and gets [`Answer::Size`], then with exactly that size and gets the data.
    pub fn query(&self, query: u32, size: u32, input: &[u8]) -> Result<Answer, Errno> {
        if query == QUERY_UC_FW_VERSION {
            return match size {
                0 => Ok(Answer::Size(32)),
                32 => self.uc_fw_version(input).map(Answer::Data),
                _ => Err(Errno::Inval),
            };
        }
        let data = match query {
            QUERY_ENGINES => self.engines(),
            QUERY_MEM_REGIONS => self.mem_regions(),
            QUERY_CONFIG => self.config(),
            QUERY_GT_LIST => self.gt_list(),
            QUERY_HWCONFIG => self.hwconfig.clone(),
            _ => return Err(Errno::Inval),
        };
        match size {
            0 => Ok(Answer::Size(data.len() as u32)),
            n if n as usize == data.len() => Ok(Answer::Data(data)),
            _ => Err(Errno::Inval),
        }
    }
}

/// What a device query brings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The size the program's buffer must have (written to the request's `size`).
    Size(u32),
    /// The bytes for the program's buffer.
    Data(Vec<u8>),
}

/// The fields of `struct drm_xe_device_query` a program filled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceQuery {
    /// Which query.
    pub query: u32,
    /// Size of the program's buffer.
    pub size: u32,
    /// Address of the program's buffer.
    pub data: u64,
}

impl DeviceQuery {
    /// Reads the request structure; extensions and reserved fields must be zero.
    pub fn parse(arg: &[u8]) -> Result<DeviceQuery, Errno> {
        let b = arg.get(..DEVICE_QUERY_SIZE).ok_or(Errno::Inval)?;
        let u64_at = |at: usize| {
            let mut w = [0u8; 8];
            w.copy_from_slice(&b[at..at + 8]);
            u64::from_le_bytes(w)
        };
        if u64_at(0) != 0 || u64_at(24) != 0 || u64_at(32) != 0 {
            return Err(Errno::Inval);
        }
        Ok(DeviceQuery {
            query: u64_at(8) as u32,
            size: (u64_at(8) >> 32) as u32,
            data: u64_at(16),
        })
    }
}

#[cfg(test)]
mod tests;
