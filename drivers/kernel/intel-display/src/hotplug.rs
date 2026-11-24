//! Hot-plug detection on display 14. Three sources, by port kind:
//!
//! * Type-C port wired to a fixed DP/HDMI connector ("legacy"): the PCH pin in `SDEISR`;
//! * Type-C port in DP-alt or Thunderbolt mode: the PICA live status;
//! * ports A/B: the PCH pin in `SDEISR`.
//!
//! The driver enables detection on every port it serves and compares the live status with what it
//! saw last (from its polling path): a change on a port is one hot-plug event for its connectors.

use crate::hw::Hw;
use crate::regs::irq;
use crate::regs::port::{self, is_tc, tc_index};

/// What is attached to a Type-C port, from the live status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TcLive {
    /// Legacy connector (fixed DP/HDMI socket) sees a sink.
    pub legacy: bool,
    /// A DP-alt mode partner is attached.
    pub dp_alt: bool,
    /// A Thunderbolt partner is attached.
    pub tbt: bool,
}

/// Enables hot-plug detection for `ports` (indices) and unmasks their identity bits.
pub fn enable<H: Hw>(hw: &mut H, ports: &[u8]) {
    let mut sde = 0;
    let mut pica = 0;
    for &p in ports {
        if is_tc(p) {
            let tc = tc_index(p);
            hw.rmw(irq::SHOTPLUG_CTL_TC, 0, irq::shotplug_tc_enable(tc));
            hw.rmw(port::hotplug_ctl(p), 0, irq::XELPDP_HOTPLUG_ENABLE);
            sde |= irq::sde_tc(tc);
            pica |= irq::pica_dp_alt(tc) | irq::pica_tbt(tc);
        } else {
            hw.rmw(irq::SHOTPLUG_CTL_DDI, 0, irq::shotplug_ddi_enable(p));
            sde |= irq::sde_ddi(p);
        }
    }
    hw.rmw(irq::SDE_IMR, sde, 0);
    hw.rmw(irq::PICA_IMR, pica, 0);
}

/// Live status of a Type-C port.
pub fn tc_live<H: Hw>(hw: &mut H, p: u8) -> TcLive {
    let tc = tc_index(p);
    let sde = hw.read(irq::SDE_ISR);
    let pica = hw.read(irq::PICA_ISR);
    TcLive {
        legacy: sde & irq::sde_tc(tc) != 0,
        dp_alt: pica & irq::pica_dp_alt(tc) != 0,
        tbt: pica & irq::pica_tbt(tc) != 0,
    }
}

/// Whether a sink is attached to `p` (any mode).
pub fn connected<H: Hw>(hw: &mut H, p: u8) -> bool {
    if is_tc(p) {
        let l = tc_live(hw, p);
        l.legacy || l.dp_alt || l.tbt
    } else {
        hw.read(irq::SDE_ISR) & irq::sde_ddi(p) != 0
    }
}

/// Bit mask (bit = port index) of ports in `ports` that have a sink now.
pub fn live_mask<H: Hw>(hw: &mut H, ports: &[u8]) -> u32 {
    ports
        .iter()
        .filter(|p| connected(hw, **p))
        .fold(0, |m, p| m | 1 << p)
}

/// Remembers the last live mask; reports the ports whose state changed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tracker {
    last: Option<u32>,
}

impl Tracker {
    /// Feeds the current mask; returns the changed ports as a mask (0 on the first call).
    pub fn update(&mut self, mask: u32) -> u32 {
        let changed = self.last.map_or(0, |l| l ^ mask);
        self.last = Some(mask);
        changed
    }
}

/// Clears latched hot-plug identity bits (the live status is what counts).
pub fn ack<H: Hw>(hw: &mut H) {
    let sde = hw.read(irq::SDE_IIR);
    if sde != 0 {
        hw.write(irq::SDE_IIR, sde);
    }
    let pica = hw.read(irq::PICA_IIR);
    if pica != 0 {
        hw.write(irq::PICA_IIR, pica);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn captured_status_shows_tc4_legacy_sink() {
        let mut s = Sim::new();
        s.set(irq::SDE_ISR, 0x0b00_3006);
        assert_eq!(
            tc_live(&mut s, 6),
            TcLive {
                legacy: true,
                dp_alt: false,
                tbt: false
            }
        );
        assert!(connected(&mut s, 6));
        assert!(!connected(&mut s, 5));
        s.set(irq::PICA_ISR, irq::pica_dp_alt(0));
        assert!(tc_live(&mut s, 3).dp_alt);
        assert_eq!(live_mask(&mut s, &[3, 5, 6]), (1 << 3) | (1 << 6));
        s.set(irq::SDE_ISR, irq::sde_ddi(1));
        assert!(connected(&mut s, 1));
    }

    #[test]
    fn enable_sets_detection_and_unmasks() {
        let mut s = Sim::new();
        s.set(irq::SDE_IMR, u32::MAX);
        s.set(irq::PICA_IMR, u32::MAX);
        enable(&mut s, &[1, 6]);
        assert_eq!(s.get(irq::SHOTPLUG_CTL_TC), 0x8000);
        assert_eq!(s.get(irq::SHOTPLUG_CTL_DDI), 0x80);
        assert_eq!(s.get(0x16f870), irq::XELPDP_HOTPLUG_ENABLE);
        assert_eq!(s.get(irq::SDE_IMR), !(irq::sde_tc(3) | irq::sde_ddi(1)));
        assert_eq!(
            s.get(irq::PICA_IMR),
            !(irq::pica_dp_alt(3) | irq::pica_tbt(3))
        );
    }

    #[test]
    fn tracker_reports_changes() {
        let mut t = Tracker::default();
        assert_eq!(t.update(1 << 6), 0);
        assert_eq!(t.update(1 << 6), 0);
        assert_eq!(t.update(0), 1 << 6);
        assert_eq!(t.update((1 << 3) | (1 << 6)), (1 << 3) | (1 << 6));
    }

    #[test]
    fn ack_clears_latched_bits() {
        let mut s = Sim::new();
        s.set(irq::SDE_IIR, 0x10);
        ack(&mut s);
        assert_eq!(s.writes_to(irq::SDE_IIR), alloc::vec![0x10]);
        assert!(s.writes_to(irq::PICA_IIR).is_empty());
    }
}
