//! Which graphics and media IP the device has. Since Meteor Lake the hardware says so itself in
//! `GMD_ID`; the PCI device id only selects the driver. Follows `xe_pci.c` (MIT).

use crate::regs::*;
use crate::{field, Error, Hw, Result};

/// A decoded `GMD_ID`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gmdid {
    /// Architecture (major version).
    pub arch: u32,
    /// Release (minor version).
    pub release: u32,
    /// Stepping.
    pub revid: u32,
}

impl Gmdid {
    /// Decodes a register value; `None` for 0 (IP absent) and all ones (device gone).
    pub fn decode(val: u32) -> Option<Gmdid> {
        (val != 0 && val != u32::MAX).then(|| Gmdid {
            arch: field(val, GMD_ID_ARCH),
            release: field(val, GMD_ID_RELEASE),
            revid: field(val, GMD_ID_REVID),
        })
    }

    /// Version times 100 (12.74 → 1274).
    pub fn verx100(&self) -> u32 {
        self.arch * 100 + self.release
    }
}

/// An IP this driver has a description for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ip {
    /// Version times 100.
    pub verx100: u32,
    /// Intel's name of the IP.
    pub name: &'static str,
    /// Stepping.
    pub revid: u32,
}

const GRAPHICS: &[(u32, &str)] = &[(1270, "Xe_LPG"), (1271, "Xe_LPG"), (1274, "Xe_LPG+")];
const MEDIA: &[(u32, &str)] = &[(1300, "Xe_LPM+")];

fn find(table: &[(u32, &'static str)], id: Gmdid, media: bool) -> Result<Ip> {
    let v = id.verx100();
    table
        .iter()
        .find(|(ver, _)| *ver == v)
        .map(|(verx100, name)| Ip {
            verx100: *verx100,
            name,
            revid: id.revid,
        })
        .ok_or(Error::UnknownIp { verx100: v, media })
}

/// Reads the graphics IP (readable without forcewake).
pub fn graphics(hw: &mut impl Hw) -> Result<Ip> {
    let val = hw.read(GMD_ID);
    if val == u32::MAX {
        return Err(Error::DeviceGone);
    }
    let id = Gmdid::decode(val).ok_or(Error::UnknownIp {
        verx100: 0,
        media: false,
    })?;
    find(GRAPHICS, id, false)
}

/// Reads the media IP; `Ok(None)` when the device has no separate media GT.
pub fn media(hw: &mut impl Hw) -> Result<Option<Ip>> {
    let val = hw.read(GMD_ID + MEDIA_GSI_OFFSET);
    if val == u32::MAX {
        return Err(Error::DeviceGone);
    }
    match Gmdid::decode(val) {
        None => Ok(None),
        Some(id) => find(MEDIA, id, true).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    /// 12.74 stepping 4, as `(arch << 22) | (release << 14) | revid`.
    const ARL_H: u32 = (12 << 22) | (74 << 14) | 4;
    const MEDIA_13: u32 = 13 << 22;

    #[test]
    fn decodes_the_fields() {
        let id = Gmdid::decode(ARL_H).unwrap();
        assert_eq!(
            (id.arch, id.release, id.revid, id.verx100()),
            (12, 74, 4, 1274)
        );
        assert_eq!(Gmdid::decode(0), None);
        assert_eq!(Gmdid::decode(u32::MAX), None);
    }

    #[test]
    fn reads_graphics_and_media() {
        let mut s = Sim::new();
        s.set(GMD_ID, ARL_H);
        s.set(GMD_ID + MEDIA_GSI_OFFSET, MEDIA_13);
        assert_eq!(
            graphics(&mut s),
            Ok(Ip {
                verx100: 1274,
                name: "Xe_LPG+",
                revid: 4
            })
        );
        assert_eq!(
            media(&mut s),
            Ok(Some(Ip {
                verx100: 1300,
                name: "Xe_LPM+",
                revid: 0
            }))
        );
        assert!(s.writes.is_empty(), "identification must not write");
    }

    #[test]
    fn unknown_and_absent() {
        let mut s = Sim::new();
        assert_eq!(
            graphics(&mut s),
            Err(Error::UnknownIp {
                verx100: 0,
                media: false
            })
        );
        assert_eq!(media(&mut s), Ok(None));
        s.set(GMD_ID, 20 << 22);
        assert_eq!(
            graphics(&mut s),
            Err(Error::UnknownIp {
                verx100: 2000,
                media: false
            })
        );
        s.set(GMD_ID + MEDIA_GSI_OFFSET, 20 << 22);
        assert_eq!(
            media(&mut s),
            Err(Error::UnknownIp {
                verx100: 2000,
                media: true
            })
        );
        s.set(GMD_ID, u32::MAX);
        assert_eq!(graphics(&mut s), Err(Error::DeviceGone));
    }
}
