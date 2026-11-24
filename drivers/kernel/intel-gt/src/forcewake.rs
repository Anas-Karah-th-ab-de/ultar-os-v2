//! Forcewake: the GT's power domains sleep on their own; before a register of a domain is
//! touched the driver asks the domain to stay awake and waits for the acknowledge. Follows
//! `xe_force_wake.c` (MIT).

use crate::regs::*;
use crate::{Error, Hw, Result};

/// How long a domain may take to acknowledge (50 ms, as Linux).
pub const ACK_TIMEOUT_US: u32 = 50_000;

/// A power domain of one GT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    /// The GT itself (GuC, GT-wide registers, fuses).
    Gt,
    /// Render, copy and compute engines.
    Render,
    /// Video decode box `n` (0..8).
    MediaVdbox(u32),
    /// Video enhancement box `n` (0..4).
    MediaVebox(u32),
    /// The graphics security controller's engine.
    Gsc,
}

/// Number of domains a GT can have.
pub const DOMAINS: usize = 15;

impl Domain {
    /// Request register (without the GT's base).
    pub const fn ctl(self) -> u32 {
        match self {
            Domain::Gt => FORCEWAKE_GT,
            Domain::Render => FORCEWAKE_RENDER,
            Domain::MediaVdbox(n) => forcewake_media_vdbox(n),
            Domain::MediaVebox(n) => forcewake_media_vebox(n),
            Domain::Gsc => FORCEWAKE_GSC,
        }
    }

    /// Acknowledge register (without the GT's base).
    pub const fn ack(self) -> u32 {
        match self {
            Domain::Gt => FORCEWAKE_ACK_GT_MTL,
            Domain::Render => FORCEWAKE_ACK_RENDER,
            Domain::MediaVdbox(n) => forcewake_ack_media_vdbox(n),
            Domain::MediaVebox(n) => forcewake_ack_media_vebox(n),
            Domain::Gsc => FORCEWAKE_ACK_GSC,
        }
    }

    /// Position in the reference table; `None` for a box number that does not exist.
    fn index(self) -> Option<usize> {
        match self {
            Domain::Gt => Some(0),
            Domain::Render => Some(1),
            Domain::Gsc => Some(2),
            Domain::MediaVdbox(n) if n < 8 => Some(3 + n as usize),
            Domain::MediaVebox(n) if n < 4 => Some(11 + n as usize),
            _ => None,
        }
    }
}

/// Wake references of one GT. Every `get` needs its `put`; the domain is released when the last
/// reference goes.
#[derive(Clone, Debug)]
pub struct Forcewake {
    base: u32,
    refs: [u16; DOMAINS],
}

impl Forcewake {
    /// For the primary GT (`base` 0) or the media GT ([`MEDIA_GSI_OFFSET`]).
    pub const fn new(base: u32) -> Forcewake {
        Forcewake {
            base,
            refs: [0; DOMAINS],
        }
    }

    /// Whether the domain is held awake by this driver.
    pub fn held(&self, d: Domain) -> bool {
        d.index().is_some_and(|i| self.refs[i] > 0)
    }

    fn drive(&self, hw: &mut impl Hw, d: Domain, wake: bool) -> Result<()> {
        let ack = d.ack() + self.base;
        let want = if wake { FORCEWAKE_KERNEL } else { 0 };
        hw.write(d.ctl() + self.base, FORCEWAKE_KERNEL_MASK | want);
        match hw.wait(ack, FORCEWAKE_KERNEL, want, ACK_TIMEOUT_US) {
            // all ones "acknowledges" a wake although the device is gone
            Ok(u32::MAX) => Err(Error::DeviceGone),
            Ok(_) => Ok(()),
            Err(ultar_intel_display::Error::Timeout { last: u32::MAX, .. }) => {
                Err(Error::DeviceGone)
            }
            Err(ultar_intel_display::Error::Timeout { last, .. }) => Err(Error::Forcewake {
                reg: ack,
                ack: last,
            }),
            Err(e) => Err(e.into()),
        }
    }

    /// Wakes the domain (first reference) and holds it.
    pub fn get(&mut self, hw: &mut impl Hw, d: Domain) -> Result<()> {
        let i = d.index().ok_or(Error::Invalid("forcewake domain"))?;
        if self.refs[i] == 0 {
            if let Err(e) = self.drive(hw, d, true) {
                // withdraw the request so the domain is not kept awake by a failed attempt
                hw.write(d.ctl() + self.base, FORCEWAKE_KERNEL_MASK);
                return Err(e);
            }
        }
        self.refs[i] = self.refs[i].saturating_add(1);
        Ok(())
    }

    /// Drops one reference; the last one lets the domain sleep again.
    pub fn put(&mut self, hw: &mut impl Hw, d: Domain) -> Result<()> {
        let i = d.index().ok_or(Error::Invalid("forcewake domain"))?;
        match self.refs[i] {
            0 => Err(Error::Invalid("forcewake put without get")),
            1 => {
                self.refs[i] = 0;
                self.drive(hw, d, false)
            }
            _ => {
                self.refs[i] -= 1;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn wake_and_sleep_once_per_last_reference() {
        let mut s = Sim::new().with_forcewake(0);
        let mut fw = Forcewake::new(0);
        fw.get(&mut s, Domain::Gt).unwrap();
        fw.get(&mut s, Domain::Gt).unwrap();
        assert!(fw.held(Domain::Gt) && !fw.held(Domain::Render));
        assert_eq!(s.writes_to(FORCEWAKE_GT), [0x1_0001]);
        fw.put(&mut s, Domain::Gt).unwrap();
        assert_eq!(s.writes_to(FORCEWAKE_GT), [0x1_0001]);
        fw.put(&mut s, Domain::Gt).unwrap();
        assert_eq!(s.writes_to(FORCEWAKE_GT), [0x1_0001, 0x1_0000]);
        assert!(!fw.held(Domain::Gt));
        assert_eq!(
            fw.put(&mut s, Domain::Gt),
            Err(Error::Invalid("forcewake put without get"))
        );
    }

    #[test]
    fn media_gt_uses_the_moved_registers() {
        let mut s = Sim::new().with_forcewake(MEDIA_GSI_OFFSET);
        let mut fw = Forcewake::new(MEDIA_GSI_OFFSET);
        fw.get(&mut s, Domain::MediaVdbox(2)).unwrap();
        assert_eq!(s.writes, [(0xa548 + MEDIA_GSI_OFFSET, 0x1_0001)]);
        assert_eq!(s.get(0xd58 + MEDIA_GSI_OFFSET), 1);
        assert_eq!(
            fw.get(&mut s, Domain::MediaVdbox(8)),
            Err(Error::Invalid("forcewake domain"))
        );
    }

    #[test]
    fn no_acknowledge_is_an_error_and_withdraws_the_request() {
        let mut s = Sim::new();
        let mut fw = Forcewake::new(0);
        assert_eq!(
            fw.get(&mut s, Domain::Render),
            Err(Error::Forcewake {
                reg: FORCEWAKE_ACK_RENDER,
                ack: 0
            })
        );
        assert!(!fw.held(Domain::Render));
        assert_eq!(s.writes_to(FORCEWAKE_RENDER), [0x1_0001, 0x1_0000]);
        assert!(s.now_us() >= ACK_TIMEOUT_US as u64);
    }

    #[test]
    fn all_ones_means_the_device_is_gone() {
        let mut s = Sim::new();
        s.read_only(FORCEWAKE_ACK_GT_MTL);
        s.set(FORCEWAKE_ACK_GT_MTL, u32::MAX);
        let mut fw = Forcewake::new(0);
        assert_eq!(fw.get(&mut s, Domain::Gt), Err(Error::DeviceGone));
    }
}
