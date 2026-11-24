//! A read-out of the GT: what the device is, which engines it has and where its GuC stands.
//! The only writes are forcewake requests (taken and given back here).

use crate::engines::{self, Engines};
use crate::forcewake::{Domain, Forcewake};
use crate::guc::Status;
use crate::ident::{self, Ip};
use crate::regs::*;
use crate::{wopcm, Error, Hw, Result};
use core::fmt;

/// What the read-out found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GtFacts {
    /// Graphics IP of the primary GT.
    pub graphics: Ip,
    /// Engines of the primary GT.
    pub engines: Engines,
    /// Status of the primary GT's GuC.
    pub guc: Status,
    /// The GuC's part of the protected memory `(base, size)` if its registers are locked.
    pub wopcm: Option<(u32, u32)>,
    /// The media GT, if the device has one.
    pub media: Option<MediaFacts>,
}

/// The media GT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaFacts {
    /// Media IP.
    pub ip: Ip,
    /// Engines and GuC status, or why the media GT did not wake.
    pub state: core::result::Result<(Engines, Status), Error>,
}

fn with_gt<H: Hw, T>(hw: &mut H, base: u32, f: impl FnOnce(&mut H) -> T) -> Result<T> {
    let mut fw = Forcewake::new(base);
    fw.get(hw, Domain::Gt)?;
    let out = f(hw);
    fw.put(hw, Domain::Gt)?;
    Ok(out)
}

/// Reads the state of the GT. Fails only if the primary GT cannot be identified or woken.
pub fn read(hw: &mut impl Hw) -> Result<GtFacts> {
    let graphics = ident::graphics(hw)?;
    let (guc, wopcm) = with_gt(hw, 0, |h| (Status::read(h), wopcm::locked(h)))?;
    let media = match ident::media(hw) {
        Ok(None) => None,
        Ok(Some(ip)) => {
            let state = with_gt(hw, MEDIA_GSI_OFFSET, |h| {
                (
                    engines::media(h),
                    Status::decode(h.read(GUC_STATUS + MEDIA_GSI_OFFSET)),
                )
            });
            Some(MediaFacts { ip, state })
        }
        // an unknown media IP does not stop the primary GT
        Err(_) => None,
    };
    Ok(GtFacts {
        graphics,
        engines: engines::primary(&graphics),
        guc,
        wopcm,
        media,
    })
}

fn ip(f: &mut fmt::Formatter<'_>, ip: &Ip) -> fmt::Result {
    write!(
        f,
        "{}.{:02} {} rev {}",
        ip.verx100 / 100,
        ip.verx100 % 100,
        ip.name,
        ip.revid
    )
}

impl fmt::Display for GtFacts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("graphics ")?;
        ip(f, &self.graphics)?;
        write!(
            f,
            " [{}], GuC: {}",
            self.engines.names().join(" "),
            self.guc
        )?;
        match self.wopcm {
            Some((base, size)) => write!(f, ", WOPCM locked {}K+{}K", base >> 10, size >> 10)?,
            None => f.write_str(", WOPCM open")?,
        }
        if let Some(m) = &self.media {
            f.write_str("; media ")?;
            ip(f, &m.ip)?;
            match &m.state {
                Ok((e, guc)) => write!(f, " [{}], GuC: {}", e.names().join(" "), guc)?,
                Err(e) => write!(f, " ({e})")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use alloc::string::ToString;

    fn beelink() -> Sim {
        let mut s = Sim::new()
            .with_forcewake(0)
            .with_forcewake(MEDIA_GSI_OFFSET);
        s.set(GMD_ID, (12 << 22) | (74 << 14) | 4);
        s.set(GMD_ID + MEDIA_GSI_OFFSET, 13 << 22);
        s.set(GUC_STATUS, 0x8002_f0ec);
        s.set(GUC_STATUS + MEDIA_GSI_OFFSET, GS_MIA_IN_RESET);
        s.set(GUC_WOPCM_SIZE, 0x1f_0000 | GUC_WOPCM_SIZE_LOCKED);
        s.set(DMA_GUC_WOPCM_OFFSET, 0x10_0000 | GUC_WOPCM_OFFSET_VALID);
        s.set(GT_VEBOX_VDBOX_DISABLE + MEDIA_GSI_OFFSET, 0x1_0005);
        s
    }

    #[test]
    fn reads_both_gts_and_releases_forcewake() {
        let mut s = beelink();
        let f = read(&mut s).unwrap();
        assert_eq!(f.graphics.verx100, 1274);
        assert_eq!(f.engines.count(), 3);
        assert_eq!(f.wopcm, Some((0x10_0000, 0x1f_0000)));
        let m = f.media.unwrap();
        assert_eq!(m.ip.verx100, 1300);
        assert_eq!(m.state.unwrap().0.count(), 4);
        // only forcewake was written, and every wake has its sleep
        assert_eq!(
            s.writes,
            [
                (FORCEWAKE_GT, 0x1_0001),
                (FORCEWAKE_GT, 0x1_0000),
                (FORCEWAKE_GT + MEDIA_GSI_OFFSET, 0x1_0001),
                (FORCEWAKE_GT + MEDIA_GSI_OFFSET, 0x1_0000),
            ]
        );
        let t = f.to_string();
        assert!(
            t.starts_with("graphics 12.74 Xe_LPG+ rev 4 [rcs0 bcs0 ccs0], GuC: boot ROM 0x76"),
            "{t}"
        );
        assert!(t.contains("WOPCM locked 1024K+1984K"), "{t}");
        assert!(
            t.contains("media 13.00 Xe_LPM+ rev 0 [vcs0 vcs2 vecs0 gsccs0]"),
            "{t}"
        );
    }

    #[test]
    fn a_sleeping_media_gt_does_not_fail_the_readout() {
        let mut s = Sim::new().with_forcewake(0);
        s.set(GMD_ID, (12 << 22) | (70 << 14));
        s.set(GMD_ID + MEDIA_GSI_OFFSET, 13 << 22);
        let f = read(&mut s).unwrap();
        assert!(matches!(
            f.media.unwrap().state,
            Err(Error::Forcewake { .. })
        ));
        assert!(f.to_string().contains("WOPCM open"));
        assert!(f.to_string().contains("forcewake not acknowledged"));
    }

    #[test]
    fn primary_gt_must_answer() {
        let mut s = Sim::new();
        s.set(GMD_ID, (12 << 22) | (74 << 14));
        assert!(matches!(read(&mut s), Err(Error::Forcewake { .. })));
        let mut s = Sim::new();
        s.set(GMD_ID, u32::MAX);
        assert_eq!(read(&mut s), Err(Error::DeviceGone));
    }
}
