//! Virtual input devices through `dev/uinput` (Linux): a keyboard or a mouse that a
//! service feeds with `struct input_event` records – how the Bluetooth service hands
//! keyboards and mice to the input subsystem on Linux (development). The Ultar kernel
//! has no `uinput`: [`VirtualInput::create`] fails there with `NotFound`.
//!
//! Five ioctls (`UI_SET_EVBIT`, `UI_SET_KEYBIT`, `UI_SET_RELBIT` with an integer argument,
//! `UI_DEV_CREATE`, `UI_DEV_DESTROY` without one): no pointer reaches the kernel.
#![allow(unsafe_code)]

use rustix::ioctl::{ioctl, opcode, IntegerSetter, NoArg, Opcode};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const UI_DEV_CREATE: Opcode = opcode::none(b'U', 1);
const UI_DEV_DESTROY: Opcode = opcode::none(b'U', 2);
const UI_SET_EVBIT: Opcode = opcode::write::<i32>(b'U', 100);
const UI_SET_KEYBIT: Opcode = opcode::write::<i32>(b'U', 101);
const UI_SET_RELBIT: Opcode = opcode::write::<i32>(b'U', 102);
/// `sizeof(struct uinput_user_dev)`.
pub const USER_DEV_SIZE: usize = 80 + 8 + 4 + 4 * 64 * 4;

/// What the device can send.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// Keys 1..=255 (Linux key codes).
    Keyboard,
    /// Three buttons, X/Y and the wheel.
    Mouse,
}

/// A virtual input device (destroyed on drop).
pub struct VirtualInput {
    file: File,
}

fn set(file: &File, which: Opcode, value: u16) -> std::io::Result<()> {
    match which {
        UI_SET_EVBIT => {
            // SAFETY: the argument is an integer, not a pointer.
            unsafe {
                ioctl(
                    file,
                    IntegerSetter::<UI_SET_EVBIT>::new_usize(usize::from(value)),
                )
            }?
        }
        UI_SET_KEYBIT => {
            // SAFETY: the argument is an integer, not a pointer.
            unsafe {
                ioctl(
                    file,
                    IntegerSetter::<UI_SET_KEYBIT>::new_usize(usize::from(value)),
                )
            }?
        }
        _ => {
            // SAFETY: the argument is an integer, not a pointer.
            unsafe {
                ioctl(
                    file,
                    IntegerSetter::<UI_SET_RELBIT>::new_usize(usize::from(value)),
                )
            }?
        }
    }
    Ok(())
}

/// The legacy setup record: name, `BUS_BLUETOOTH` (5), no axes with ranges.
pub fn user_dev(name: &str) -> Vec<u8> {
    let mut rec = vec![0u8; USER_DEV_SIZE];
    let n = name.as_bytes();
    let len = n.len().min(79);
    rec[..len].copy_from_slice(&n[..len]);
    rec[80..82].copy_from_slice(&5u16.to_ne_bytes());
    rec[82..84].copy_from_slice(&0x1d6bu16.to_ne_bytes());
    rec
}

/// One `struct input_event` (64-bit layout, time left 0: the kernel stamps it).
pub fn event_record(kind: u16, code: u16, value: i32) -> [u8; 24] {
    let mut r = [0u8; 24];
    r[16..18].copy_from_slice(&kind.to_ne_bytes());
    r[18..20].copy_from_slice(&code.to_ne_bytes());
    r[20..24].copy_from_slice(&value.to_ne_bytes());
    r
}

impl VirtualInput {
    /// Creates a device below `root` (`dev/uinput`).
    pub fn create(root: &Path, shape: Shape, name: &str) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(root.join("dev/uinput"))?;
        set(&file, UI_SET_EVBIT, 1)?; // EV_KEY
        match shape {
            Shape::Keyboard => {
                for k in 1..=255 {
                    set(&file, UI_SET_KEYBIT, k)?;
                }
            }
            Shape::Mouse => {
                set(&file, UI_SET_EVBIT, 2)?; // EV_REL
                for b in 0x110..=0x112 {
                    set(&file, UI_SET_KEYBIT, b)?;
                }
                for r in [0, 1, 8] {
                    set(&file, UI_SET_RELBIT, r)?;
                }
            }
        }
        (&file).write_all(&user_dev(name))?;
        // SAFETY: no argument.
        unsafe { ioctl(&file, NoArg::<UI_DEV_CREATE>::new()) }?;
        Ok(Self { file })
    }

    /// Sends events (the caller ends them with `SYN_REPORT`).
    pub fn send(&mut self, events: &[(u16, u16, i32)]) -> std::io::Result<()> {
        let mut buf = Vec::with_capacity(events.len() * 24);
        for &(k, c, v) in events {
            buf.extend_from_slice(&event_record(k, c, v));
        }
        self.file.write_all(&buf)
    }
}

impl Drop for VirtualInput {
    fn drop(&mut self) {
        // SAFETY: no argument.
        let _ = unsafe { ioctl(&self.file, NoArg::<UI_DEV_DESTROY>::new()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_have_the_kernel_layout() {
        assert_eq!(USER_DEV_SIZE, 1116);
        let rec =
            user_dev("Keyboard K380 with a name longer than the kernel allows for a device, cut");
        assert_eq!(rec.len(), 1116);
        assert_eq!(rec[79], 0, "name is NUL-terminated");
        assert_eq!(u16::from_ne_bytes([rec[80], rec[81]]), 5);
        let e = event_record(1, 30, 1);
        assert_eq!(&e[..16], &[0; 16]);
        assert_eq!(u16::from_ne_bytes([e[18], e[19]]), 30);
        let root = std::env::temp_dir().join(format!("hal-uinput-{}", std::process::id()));
        assert!(
            VirtualInput::create(&root, Shape::Keyboard, "x").is_err(),
            "no uinput: refused"
        );
    }
}
