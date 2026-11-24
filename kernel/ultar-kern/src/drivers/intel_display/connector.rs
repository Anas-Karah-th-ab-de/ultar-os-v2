//! Connectors of the running device: hot-plug changes and probing (connection, DPCD, EDID over
//! AUX or GMBUS).

use super::device::{Device, Output};
use super::mmio::Mmio;
use alloc::vec::Vec;
use ultar_intel_display::dp::{aux, dpcd};
use ultar_intel_display::regs::port;
use ultar_intel_display::{edid, gmbus, hotplug, modeset, Error, Hw};

/// What probing a connector found.
#[derive(Clone, Debug, Default)]
pub struct Probe {
    /// A sink is attached.
    pub connected: bool,
    /// Its EDID (base block + extensions), when it could be read.
    pub edid: Option<Vec<u8>>,
    /// DPCD capabilities (DP sinks).
    pub caps: Option<dpcd::Caps>,
}

impl Device {
    /// Ports whose connection changed since the last call (bit = port index).
    pub fn poll_hotplug(&mut self) -> u32 {
        if !self.plat.is_xelpdp() {
            return 0;
        }
        hotplug::ack(&mut self.mmio);
        let ports: Vec<u8> = self.outputs.iter().map(|o| o.port).collect();
        let mask = hotplug::live_mask(&mut self.mmio, &ports);
        self.hotplug.update(mask)
    }

    /// Reads connection, DPCD and EDID of output `o`.
    pub fn probe(&mut self, o: Output) -> Probe {
        let ver = self.plat.display_ver;
        if !self.plat.is_xelpdp() {
            return Probe::default();
        }
        let connected = hotplug::connected(&mut self.mmio, o.port);
        if !connected {
            return Probe::default();
        }
        let mut pr = Probe {
            connected,
            ..Default::default()
        };
        if o.dp {
            let owned = self.mmio.read(port::buf_ctl1(o.port)) & port::BUF1_TC_PHY_OWNERSHIP != 0;
            if !owned && port::is_tc(o.port) && modeset::tc_connect(&mut self.mmio, o.port).is_err()
            {
                return pr;
            }
            aux::power_on(&mut self.mmio, o.port);
            pr.caps = modeset::read_caps(&mut self.mmio, o.port, ver).ok();
            if pr.caps.is_some() {
                pr.edid = self.read_edid(|hw, seg, off, buf| {
                    aux::i2c_read(hw, o.port, ver, 0x50, seg, off, buf)
                });
            }
        }
        if pr.edid.is_none() && o.hdmi && o.ddc_pin != 0 {
            pr.edid =
                self.read_edid(|hw, _seg, off, buf| gmbus::read(hw, o.ddc_pin, 0x50, off, buf));
        }
        pr
    }

    /// Reads and checks an EDID (base block and the first extension block, if any).
    fn read_edid(
        &mut self,
        mut rd: impl FnMut(&mut Mmio, Option<u8>, u8, &mut [u8]) -> Result<(), Error>,
    ) -> Option<Vec<u8>> {
        let mut base = [0u8; 128];
        rd(&mut self.mmio, None, 0, &mut base).ok()?;
        let ext = edid::check_base(&base).ok()?.min(1);
        let mut all = base.to_vec();
        if ext == 1 {
            let mut e = [0u8; 128];
            rd(&mut self.mmio, None, 128, &mut e).ok()?;
            all.extend_from_slice(&e);
        }
        if base[126] > 1 {
            // only the first extension is read: the image says one extension
            edid::claim_extensions(&mut all[..128], 1);
        }
        edid::parse(&all).ok()?;
        Some(all)
    }
}
