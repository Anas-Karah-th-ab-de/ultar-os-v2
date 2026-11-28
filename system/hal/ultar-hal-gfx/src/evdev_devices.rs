//! The set of open event devices: reading, removal of devices that are
//! gone, adding devices that appear (hot-plug).

use super::*;

struct Device {
    path: PathBuf,
    /// Identity of the node that was opened (`st_rdev`, `st_ino`): a node
    /// replugged under the same name between two scans is another device.
    node: (u64, u64),
    info: InputDeviceInfo,
    file: File,
    decoder: Decoder,
    partial: Vec<u8>,
}

/// Open devices and the next device number (numbers are never reused, so
/// a replugged device is a new device).
#[derive(Default)]
struct Set {
    devices: Vec<Device>,
    next_id: u32,
    policy: SeatPolicy,
}

/// Which event devices belong to the seat (the compositor's user session).
///
/// `ultar-devd` gives the event nodes of the seat's devices to one group
/// (mode `0660`); with `group` set, a node of another group is not opened.
/// Devices made by software (`uinput`: below `/devices/virtual/` in sysfs,
/// or reporting `BUS_VIRTUAL`, which the Ultar kernel sets for every
/// `uinput` device whatever its maker declared – review S-4) could type
/// anything, the secure attention keys included, so they are refused unless
/// their name is in `virtual_allowed` (on-screen keyboards of the system,
/// test rigs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeatPolicy {
    /// Group the seat's event nodes must belong to (`None`: any).
    pub group: Option<u32>,
    /// Device names (`EVIOCGNAME`/sysfs `name`) of software devices that
    /// are accepted.
    pub virtual_allowed: Vec<String>,
}

/// Where sysfs lists input class devices (to tell software devices apart).
const SYS_INPUT: &str = "/sys/class/input";
/// `BUS_VIRTUAL` of `struct input_id`.
const BUS_VIRTUAL: u16 = 6;

impl SeatPolicy {
    /// May the opened node at `path` be read as a seat device?
    pub(crate) fn admits(&self, path: &Path, file: &File) -> bool {
        use std::os::unix::fs::MetadataExt;
        if let Some(group) = self.group {
            if file.metadata().map_or(true, |m| m.gid() != group) {
                return false;
            }
        }
        let Some(name) = path.file_name() else {
            return false;
        };
        let class = Path::new(SYS_INPUT).join(name);
        let software = std::fs::read_link(&class)
            .is_ok_and(|target| target.to_string_lossy().contains("/devices/virtual/"))
            || bus_of(file) == Some(BUS_VIRTUAL);
        if !software {
            return true;
        }
        let label = std::fs::read_to_string(class.join("device/name")).unwrap_or_default();
        self.virtual_allowed.iter().any(|n| n == label.trim())
    }
}

/// The identity of the node at `path` now (`None`: gone).
fn node_of(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).ok()?;
    Some((m.rdev(), m.ino()))
}

/// All event devices of the machine.
pub struct Evdev {
    set: Mutex<Set>,
}

impl std::fmt::Debug for Evdev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Evdev")
    }
}

const fn ioc_read(nr: u32, len: u32) -> rustix::ioctl::Opcode {
    (0x8000_0000 | (len << 16) | (0x45 << 8) | nr) as rustix::ioctl::Opcode
}

/// Issues one reading `ioctl` whose opcode is a constant.
macro_rules! read_ioctl {
    ($file:expr, $nr:expr, $len:expr, $value:expr) => {
        // SAFETY: an `EVIOCG*` request of `$len` bytes; the kernel writes at
        // most that many bytes into `$value`, whose type has exactly that size.
        unsafe {
            rustix::ioctl::ioctl(
                $file,
                rustix::ioctl::Updater::<{ ioc_read($nr, $len) }, _>::new($value),
            )
        }
        .is_ok()
    };
}

/// The bus of the opened event node (`EVIOCGID`), `None` if it does not say.
fn bus_of(file: &File) -> Option<u16> {
    let mut id = [0u8; 8];
    read_ioctl!(file, 0x02, 8, &mut id).then(|| u16::from_le_bytes([id[0], id[1]]))
}

fn bits(file: &File, event_type: u16, out: &mut [u8; 96]) -> bool {
    match event_type {
        0 => read_ioctl!(file, 0x20, 96, out),
        EV_KEY => read_ioctl!(file, 0x21, 96, out),
        _ => read_ioctl!(file, 0x23, 96, out),
    }
}

fn axis(file: &File, code: u16) -> Option<AxisRange> {
    let mut info = [0i32; 6];
    let ok = match code {
        ABS_X => read_ioctl!(file, 0x40, 24, &mut info),
        ABS_Y => read_ioctl!(file, 0x41, 24, &mut info),
        ABS_MT_POSITION_X => read_ioctl!(file, 0x75, 24, &mut info),
        ABS_MT_POSITION_Y => read_ioctl!(file, 0x76, 24, &mut info),
        _ => false,
    };
    ok.then_some(AxisRange {
        min: info[1],
        max: info[2],
    })
}

fn has(bits: &[u8; 96], bit: u16) -> bool {
    bits.get(usize::from(bit / 8))
        .is_some_and(|b| b & (1 << (bit % 8)) != 0)
}

/// Decides the kind of a device from its capability bits; `None` when the
/// kernel does not answer `EVIOCGBIT` (the Ultar kernel).
fn classify(file: &File) -> Option<(InputDeviceKind, bool)> {
    let (mut types, mut keys, mut abs) = ([0u8; 96], [0u8; 96], [0u8; 96]);
    if !bits(file, 0, &mut types) {
        return None;
    }
    bits(file, EV_KEY, &mut keys);
    bits(file, EV_ABS, &mut abs);
    let kind = if has(&types, EV_ABS) && has(&abs, ABS_MT_SLOT) {
        (InputDeviceKind::Touchscreen, true)
    } else if has(&types, EV_ABS) && has(&keys, BTN_TOUCH) {
        (InputDeviceKind::Touchscreen, false)
    } else if has(&types, EV_REL) || (has(&types, EV_ABS) && has(&keys, BTN_LEFT)) {
        (InputDeviceKind::Mouse, false)
    } else if has(&keys, 30) {
        (InputDeviceKind::Keyboard, false)
    } else {
        (InputDeviceKind::Buttons, false)
    };
    Some(kind)
}

impl Evdev {
    /// Opens every readable `event*` device below [`INPUT_DIR`] that the
    /// default [`SeatPolicy`] admits (no software devices).
    pub fn open_all() -> Result<Evdev, GfxError> {
        Evdev::open_seat(SeatPolicy::default())
    }

    /// Opens the seat's devices under `policy`; an empty set is fine
    /// (devices may come later with [`Evdev::rescan`]).
    pub fn open_seat(policy: SeatPolicy) -> Result<Evdev, GfxError> {
        let evdev = Evdev::empty();
        if let Ok(mut set) = evdev.set.lock() {
            set.policy = policy;
        }
        evdev.add_paths(&existing_paths());
        Ok(evdev)
    }

    /// Opens the given event devices; unreadable ones are skipped. Fails
    /// when none could be opened.
    pub fn open_paths(paths: &[PathBuf]) -> Result<Evdev, GfxError> {
        let evdev = Evdev::empty();
        evdev.add_paths(paths);
        if evdev.count() == 0 {
            return Err(GfxError::NoDevice);
        }
        Ok(evdev)
    }

    /// No device yet (devices come with [`Evdev::rescan`]).
    pub fn empty() -> Evdev {
        Evdev {
            set: Mutex::new(Set::default()),
        }
    }

    fn count(&self) -> usize {
        self.set.lock().map_or(0, |s| s.devices.len())
    }

    /// Opens the paths that are not open yet; returns `DeviceAdded` events.
    fn add_paths(&self, paths: &[PathBuf]) -> Vec<InputEvent> {
        let Ok(mut set) = self.set.lock() else {
            return Vec::new();
        };
        let mut added = Vec::new();
        for path in paths {
            if set.devices.len() >= MAX_DEVICES || set.devices.iter().any(|d| &d.path == path) {
                continue;
            }
            let id = InputDeviceId(set.next_id);
            let device = open_device(path, id).filter(|d| set.policy.admits(path, &d.file));
            if let Some(device) = device {
                set.next_id = set.next_id.wrapping_add(1);
                added.push(InputEvent {
                    device: id,
                    time_us: 0,
                    kind: InputEventKind::DeviceAdded,
                });
                set.devices.push(device);
            }
        }
        added
    }

    /// Hot-plug: drops devices whose node is gone or was replaced (same
    /// name, other node), opens new `event*` nodes. Call on a uevent, after
    /// lost uevents, or periodically where there are none.
    pub fn rescan(&self) -> Vec<InputEvent> {
        let paths = existing_paths();
        let mut events = Vec::new();
        if let Ok(mut set) = self.set.lock() {
            let live = |d: &Device| paths.contains(&d.path) && node_of(&d.path) == Some(d.node);
            for device in set.devices.iter_mut().filter(|d| !live(d)) {
                retire(device, &mut events);
            }
            set.devices.retain(live);
        }
        events.extend(self.add_paths(&paths));
        events
    }

    /// Duplicated descriptors of the live devices for the caller's `poll`
    /// loop (readable = [`Input::poll`] has events). Call again after
    /// `DeviceAdded`/`DeviceRemoved`.
    pub fn poll_handles(&self) -> Vec<OwnedFd> {
        let Ok(set) = self.set.lock() else {
            return Vec::new();
        };
        set.devices
            .iter()
            .filter_map(|d| d.file.try_clone().ok().map(OwnedFd::from))
            .collect()
    }
}

/// The `event*` nodes that exist now (every number, in number order).
fn existing_paths() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(INPUT_DIR) else {
        return Vec::new();
    };
    let mut found: Vec<(u32, PathBuf)> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name();
            let n = name.to_str()?.strip_prefix("event")?.parse().ok()?;
            Some((n, e.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

fn open_device(path: &Path, id: InputDeviceId) -> Option<Device> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
        .ok()?;
    let x_axis = axis(&file, ABS_X);
    let (kind, multitouch) = classify(&file).unwrap_or(if x_axis.is_some() {
        (InputDeviceKind::Mouse, false)
    } else {
        (InputDeviceKind::Keyboard, false)
    });
    let (x_code, y_code) = if multitouch {
        (ABS_MT_POSITION_X, ABS_MT_POSITION_Y)
    } else {
        (ABS_X, ABS_Y)
    };
    let unit = AxisRange { min: 0, max: 65535 };
    let x = axis(&file, x_code).unwrap_or(unit);
    let y = axis(&file, y_code).unwrap_or(unit);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let node = file.metadata().map_or((0, 0), |m| {
        use std::os::unix::fs::MetadataExt;
        (m.rdev(), m.ino())
    });
    Some(Device {
        path: path.to_path_buf(),
        node,
        info: InputDeviceInfo {
            id,
            kind,
            name,
            internal: true,
        },
        file,
        decoder: Decoder::new(id, x, y, multitouch),
        partial: Vec::new(),
    })
}

/// A device goes away: its held keys and buttons are released (the
/// `SYN_DROPPED` path) and `DeviceRemoved` is reported.
fn retire(device: &mut Device, out: &mut Vec<InputEvent>) {
    let mut dropped = [0u8; RECORD];
    dropped[18..20].copy_from_slice(&SYN_DROPPED.to_ne_bytes());
    device.decoder.feed(&dropped, out);
    out.push(InputEvent {
        device: device.info.id,
        time_us: 0,
        kind: InputEventKind::DeviceRemoved,
    });
}

/// Reads what a device has; false when it is gone (end of file or an error
/// other than "nothing now": an unplugged device reports `ENODEV` and
/// `POLLHUP` for ever and must leave the poll set).
fn drain(device: &mut Device, buf: &mut [u8], out: &mut Vec<InputEvent>) -> bool {
    loop {
        match device.file.read(buf) {
            Ok(0) => return false,
            Ok(n) => {
                device.partial.extend_from_slice(&buf[..n]);
                let whole = device.partial.len() / RECORD * RECORD;
                for chunk in device.partial[..whole].chunks_exact(RECORD) {
                    let mut record = [0u8; RECORD];
                    record.copy_from_slice(chunk);
                    device.decoder.feed(&record, out);
                }
                device.partial.drain(..whole);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return true,
            Err(_) => return false,
        }
    }
}

impl Input for Evdev {
    fn devices(&self) -> HalResult<Vec<InputDeviceInfo>> {
        let set = self.set.lock().map_err(|_| HalError::Busy)?;
        Ok(set.devices.iter().map(|d| d.info.clone()).collect())
    }

    /// Reads what is there without blocking (`timeout_ms` is not waited:
    /// the compositor waits on [`Evdev::poll_handles`]). A device that is
    /// gone is dropped: its held keys are released (`SYN_DROPPED` path)
    /// and `DeviceRemoved` is reported.
    fn poll(&self, _timeout_ms: u32) -> HalResult<Vec<InputEvent>> {
        let mut set = self.set.lock().map_err(|_| HalError::Busy)?;
        let mut out = Vec::new();
        let mut buf = [0u8; RECORD * 64];
        let mut gone = Vec::new();
        for (index, device) in set.devices.iter_mut().enumerate() {
            if !drain(device, &mut buf, &mut out) {
                retire(device, &mut out);
                gone.push(index);
            }
        }
        for index in gone.into_iter().rev() {
            set.devices.remove(index);
        }
        Ok(out)
    }
}
