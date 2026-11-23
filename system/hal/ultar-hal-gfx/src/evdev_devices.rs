//! The set of open event devices: reading, removal of devices that are
//! gone, adding devices that appear (hot-plug).

use super::*;

struct Device {
    path: PathBuf,
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
    /// Opens every readable `event*` device below [`INPUT_DIR`].
    pub fn open_all() -> Result<Evdev, GfxError> {
        Evdev::open_paths(&existing_paths())
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
            if let Some(device) = open_device(path, id) {
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

    /// Hot-plug: drops devices whose node is gone, opens new `event*`
    /// nodes. Call on a uevent (or periodically where there are none).
    pub fn rescan(&self) -> Vec<InputEvent> {
        let paths = existing_paths();
        let mut events = Vec::new();
        if let Ok(mut set) = self.set.lock() {
            for device in set.devices.iter_mut().filter(|d| !paths.contains(&d.path)) {
                retire(device, &mut events);
            }
            set.devices.retain(|d| paths.contains(&d.path));
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

/// The `event*` nodes that exist now.
fn existing_paths() -> Vec<PathBuf> {
    (0..MAX_DEVICES)
        .map(|index| Path::new(INPUT_DIR).join(format!("event{index}")))
        .filter(|p| p.exists())
        .collect()
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
    Some(Device {
        path: path.to_path_buf(),
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
