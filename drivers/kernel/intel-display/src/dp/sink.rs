//! Test sinks: [`FakeSink`] answers real AUX register traffic of the simulated device (DPCD,
//! EDID over I2C, DEFER/NACK on demand, link training that succeeds at once); [`Scripted`] plays
//! a sink at the level of the training algorithm.

use super::aux::{self, I2C_MOT, NATIVE_READ, NATIVE_WRITE};
use super::training::{Level, Trainer};
use crate::error::Result;
use crate::hw::{field, prep};
use crate::regs::ddi::TrainPattern;
use crate::regs::port::{self, aux as auxbits};
use crate::sim::Sim;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

/// A 1080p monitor's EDID (base block + one CTA extension), checksums valid.
pub fn test_edid() -> Vec<u8> {
    let mut e = vec![0u8; 256];
    e[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
    e[8..10].copy_from_slice(&[0x10, 0xac]); // "DEL"
    e[18] = 1;
    e[19] = 4;
    e[21] = 70; // 70 cm
    e[22] = 39;
    // preferred detailed timing: 1920x1080@60, 148.5 MHz
    let dtd = [
        0x02, 0x3a, 0x80, 0x18, 0x71, 0x38, 0x2d, 0x40, 0x58, 0x2c, 0x45, 0x00, 0xbc, 0x86, 0x21,
        0x00, 0x00, 0x1e,
    ];
    e[54..72].copy_from_slice(&dtd);
    // monitor name descriptor
    e[72..77].copy_from_slice(&[0, 0, 0, 0xfc, 0]);
    e[77..90].copy_from_slice(b"ULTAR TEST\n  ");
    e[126] = 1;
    e[127] = checksum(&e[..127]);
    e[128] = 0x02;
    e[129] = 3;
    e[130] = 4; // no data blocks, DTDs from byte 4
                // second DTD in the extension: 1280x720@60, 74.25 MHz
    let dtd2 = [
        0x01, 0x1d, 0x00, 0x72, 0x51, 0xd0, 0x1e, 0x20, 0x6e, 0x28, 0x55, 0x00, 0xbc, 0x86, 0x21,
        0x00, 0x00, 0x1e,
    ];
    e[132..150].copy_from_slice(&dtd2);
    e[255] = checksum(&e[128..255]);
    e
}

fn checksum(b: &[u8]) -> u8 {
    0u8.wrapping_sub(b.iter().fold(0u8, |a, x| a.wrapping_add(*x)))
}

/// A DisplayPort sink behind the simulated AUX channel of one port.
pub struct FakeSink {
    /// DPCD contents.
    pub dpcd: Vec<u8>,
    /// EDID behind I2C address 0x50.
    pub edid: Vec<u8>,
    /// Answer DEFER this many more times.
    pub defer: u32,
    /// Answer NACK.
    pub nack: bool,
    /// Never trains with this lane count: the lane status reads as zero while DPCD 0x101 asks
    /// for it.
    pub fail_lanes: Option<u8>,
    /// Never trains at this link rate code (DPCD 0x100, e.g. 0x0a for HBR).
    pub fail_rate: Option<u8>,
    /// Lane counts the source asked for (DPCD 0x101 writes), in order.
    pub lane_requests: Vec<u8>,
    i2c_offset: usize,
}

impl FakeSink {
    /// Attaches a sink (DPCD 1.2, HBR2 x4, TPS3, enhanced framing) to port `p` of `sim`.
    pub fn attach(sim: &mut Sim, p: u8) -> Rc<RefCell<FakeSink>> {
        let mut dpcd = vec![0u8; 0x3000];
        dpcd[..4].copy_from_slice(&[0x12, 0x0a, 0xc4, 0x01]);
        let sink = Rc::new(RefCell::new(FakeSink {
            dpcd,
            edid: test_edid(),
            defer: 0,
            nack: false,
            fail_lanes: None,
            fail_rate: None,
            lane_requests: Vec::new(),
            i2c_offset: 0,
        }));
        let s2 = sink.clone();
        sim.on_write(
            port::aux_ctl(p),
            Box::new(move |regs, reg, v| s2.borrow_mut().on_ctl(regs, p, reg, v)),
        );
        sink
    }

    fn on_ctl(&mut self, regs: &mut BTreeMap<u32, u32>, p: u8, reg: u32, v: u32) {
        let clear = auxbits::DONE | auxbits::TIME_OUT_ERROR | auxbits::RECEIVE_ERROR;
        if v & auxbits::SEND_BUSY == 0 {
            regs.insert(reg, v & !clear);
            return;
        }
        let n = field(v, auxbits::SIZE_MASK) as usize;
        let mut msg = [0u8; 20];
        for i in 0..5u8 {
            let w = regs.get(&port::aux_data(p, i)).copied().unwrap_or(0);
            aux::unpack(w, &mut msg[i as usize * 4..i as usize * 4 + 4]);
        }
        let reply = self.answer(&msg[..n]);
        for (i, c) in reply.chunks(4).enumerate() {
            regs.insert(port::aux_data(p, i as u8), aux::pack(c));
        }
        let done = (v & !auxbits::SEND_BUSY & !clear & !auxbits::SIZE_MASK)
            | auxbits::DONE
            | prep(auxbits::SIZE_MASK, reply.len() as u32);
        regs.insert(reg, done);
    }

    fn answer(&mut self, msg: &[u8]) -> Vec<u8> {
        if self.defer > 0 {
            self.defer -= 1;
            return vec![0x20 | 0x80];
        }
        if self.nack {
            return vec![0x10 | 0x40];
        }
        let cmd = msg[0] >> 4;
        let addr = ((msg[0] as usize & 0xf) << 16) | (msg[1] as usize) << 8 | msg[2] as usize;
        let len = msg.get(3).map_or(0, |l| *l as usize + 1);
        match cmd {
            NATIVE_READ => {
                let mut r = vec![0u8];
                r.extend_from_slice(&self.dpcd[addr..addr + len]);
                let lanes = self.dpcd[0x101] & 0x1f;
                if self.fail_lanes == Some(lanes) || self.fail_rate == Some(self.dpcd[0x100]) {
                    for (i, b) in r[1..].iter_mut().enumerate() {
                        if (0x202..0x205).contains(&(addr + i)) {
                            *b = 0;
                        }
                    }
                }
                r
            }
            NATIVE_WRITE => {
                self.dpcd[addr..addr + len].copy_from_slice(&msg[4..4 + len]);
                if (addr..addr + len).contains(&0x101) {
                    self.lane_requests.push(self.dpcd[0x101] & 0x1f);
                }
                vec![0]
            }
            c if c & !I2C_MOT == 0 && msg.len() > 4 => {
                // I2C write of the offset (or segment)
                if addr == 0x50 {
                    self.i2c_offset = msg[4] as usize;
                }
                vec![0]
            }
            c if c & !I2C_MOT == 1 && len > 0 => {
                let mut r = vec![0u8];
                let o = self.i2c_offset;
                r.extend_from_slice(&self.edid[o..o + len]);
                self.i2c_offset += len;
                r
            }
            _ => vec![0],
        }
    }
}

/// A sink for the training algorithm: asks for `want` during clock recovery and `want_eq` during
/// equalisation; flags make it fail.
pub struct Scripted {
    /// DPCD.
    pub dpcd: Vec<u8>,
    /// Source patterns in order.
    pub patterns: Vec<TrainPattern>,
    /// Source levels in order.
    pub levels: Vec<Level>,
    /// Never reports clock recovery.
    pub cr_never: bool,
    /// Never reports equalisation.
    pub eq_never: bool,
    /// Loses clock recovery when equalisation starts.
    pub cr_lost_in_eq: bool,
    want_swing: u8,
    want_pre: u8,
}

impl Scripted {
    /// Sink that needs swing `swing` for clock recovery and pre-emphasis `pre` for equalisation.
    pub fn new(swing: u8, pre: u8) -> Scripted {
        Scripted {
            dpcd: vec![0; 0x800],
            patterns: Vec::new(),
            levels: Vec::new(),
            cr_never: false,
            eq_never: false,
            cr_lost_in_eq: false,
            want_swing: swing,
            want_pre: pre,
        }
    }

    fn status(&self) -> [u8; 6] {
        let lvl = self.levels.last().copied().unwrap_or((0, 0));
        let in_eq = matches!(
            self.patterns.last(),
            Some(TrainPattern::Tps2 | TrainPattern::Tps3 | TrainPattern::Tps4)
        );
        let cr = !self.cr_never && lvl.0 >= self.want_swing && !(in_eq && self.cr_lost_in_eq);
        let eq = cr && in_eq && !self.eq_never && lvl.1 >= self.want_pre;
        let nib = (cr as u8) | if eq { 6 } else { 0 };
        let adj = if in_eq {
            self.want_swing | (self.want_pre << 2)
        } else {
            self.want_swing
        };
        [
            nib | nib << 4,
            nib | nib << 4,
            eq as u8,
            0,
            adj | adj << 4,
            adj | adj << 4,
        ]
    }
}

impl Trainer for Scripted {
    fn source_pattern(&mut self, p: TrainPattern) -> Result<()> {
        self.patterns.push(p);
        Ok(())
    }
    fn source_level(&mut self, level: Level) -> Result<()> {
        self.levels.push(level);
        Ok(())
    }
    fn dpcd_write(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        self.dpcd[addr as usize..addr as usize + data.len()].copy_from_slice(data);
        Ok(())
    }
    fn dpcd_read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()> {
        if addr == 0x202 {
            buf.copy_from_slice(&self.status()[..buf.len()]);
        } else {
            buf.copy_from_slice(&self.dpcd[addr as usize..addr as usize + buf.len()]);
        }
        Ok(())
    }
    fn delay_us(&mut self, _us: u32) {}
}
