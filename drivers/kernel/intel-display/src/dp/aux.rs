//! DisplayPort AUX channel: message format (DP 1.4 §2.7), the hardware transfer through
//! `DP_AUX_CH_CTL`/`DATA` (as Linux' `intel_dp_aux.c` drives it, MIT) and the DPCD and I2C-over-AUX
//! transactions built on it (DEFER retries, 16-byte chunks, MOT).

use crate::error::{AuxError, Result};
use crate::hw::{field, prep, Hw};
use crate::regs::port::{self, aux};

/// Request: native write.
pub const NATIVE_WRITE: u8 = 0x8;
/// Request: native read.
pub const NATIVE_READ: u8 = 0x9;
/// Request: I2C write.
pub const I2C_WRITE: u8 = 0x0;
/// Request: I2C read.
pub const I2C_READ: u8 = 0x1;
/// Request flag: I2C middle-of-transaction (no stop after this message).
pub const I2C_MOT: u8 = 0x4;
/// Largest payload of one AUX message.
pub const MAX_PAYLOAD: usize = 16;

/// Reply of the sink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Accepted.
    Ack,
    /// Refused.
    Nack,
    /// Busy, ask again.
    Defer,
}

/// Builds the request header: `cmd` (4 bits), 20-bit address and, for transfers with data,
/// length − 1. Returns the header and its length (3 for an address-only I2C message).
pub fn header(cmd: u8, addr: u32, len: usize) -> ([u8; 4], usize) {
    let h = [
        (cmd << 4) | ((addr >> 16) & 0xf) as u8,
        (addr >> 8) as u8,
        addr as u8,
        len.saturating_sub(1) as u8,
    ];
    (h, if len == 0 { 3 } else { 4 })
}

/// Decodes the reply command byte: native reply for native requests, I2C reply otherwise.
pub fn reply(byte0: u8, native: bool) -> Reply {
    let cmd = byte0 >> 4;
    let r = if native { cmd & 3 } else { (cmd >> 2) & 3 };
    match r {
        0 => Reply::Ack,
        1 => Reply::Nack,
        _ => Reply::Defer,
    }
}

/// Packs up to 4 bytes big-endian into one data register.
pub fn pack(b: &[u8]) -> u32 {
    b.iter()
        .take(4)
        .enumerate()
        .fold(0, |v, (i, x)| v | (*x as u32) << (24 - 8 * i))
}

/// Unpacks one data register into up to 4 bytes.
pub fn unpack(v: u32, out: &mut [u8]) {
    for (i, o) in out.iter_mut().take(4).enumerate() {
        *o = (v >> (24 - 8 * i)) as u8;
    }
}

/// `DP_AUX_CH_CTL` value that starts a transfer of `send_len` bytes.
pub fn ctl_value(send_len: usize, display_ver: u8) -> u32 {
    let mut v =
        aux::SEND_BUSY | aux::DONE | aux::TIME_OUT_ERROR | aux::TIME_OUT_MAX | aux::RECEIVE_ERROR;
    v |= prep(aux::SIZE_MASK, send_len as u32)
        | prep(aux::FW_SYNC_MASK, 18 - 1)
        | prep(aux::SYNC_MASK, 32 - 1);
    if display_ver >= 14 {
        v |= aux::POWER_REQUEST;
    }
    v
}

/// One raw AUX transfer on `p`: sends `send` (≤ 20 bytes), returns the reply bytes in `recv`.
pub fn transfer<H: Hw>(
    hw: &mut H,
    p: u8,
    display_ver: u8,
    send: &[u8],
    recv: &mut [u8; 20],
) -> Result<usize> {
    if send.len() > 20 {
        return Err(AuxError::TooLong.into());
    }
    let ctl = port::aux_ctl(p);
    if hw.wait_clear(ctl, aux::SEND_BUSY, 3000).is_err() {
        return Err(AuxError::Busy.into());
    }
    let mut status = 0;
    for _ in 0..5 {
        for (i, chunk) in send.chunks(4).enumerate() {
            hw.write(port::aux_data(p, i as u8), pack(chunk));
        }
        hw.write(ctl, ctl_value(send.len(), display_ver));
        status = hw
            .wait_clear(ctl, aux::SEND_BUSY, 10_000)
            .unwrap_or_else(|e| match e {
                crate::Error::Timeout { last, .. } => last,
                _ => 0,
            });
        hw.write(
            ctl,
            (status & !aux::SEND_BUSY) | aux::DONE | aux::TIME_OUT_ERROR | aux::RECEIVE_ERROR,
        );
        if status & aux::TIME_OUT_ERROR != 0 {
            continue;
        }
        if status & aux::RECEIVE_ERROR != 0 {
            hw.delay_us(450);
            continue;
        }
        if status & aux::DONE != 0 {
            break;
        }
    }
    if status & aux::DONE == 0 || status & aux::TIME_OUT_ERROR != 0 {
        return Err(AuxError::Timeout.into());
    }
    if status & aux::RECEIVE_ERROR != 0 {
        return Err(AuxError::Receive.into());
    }
    let n = field(status, aux::SIZE_MASK) as usize;
    if n == 0 || n > 20 {
        return Err(AuxError::Protocol.into());
    }
    for i in 0..n.div_ceil(4) {
        let v = hw.read(port::aux_data(p, i as u8));
        unpack(v, &mut recv[i * 4..n.min(i * 4 + 4)]);
    }
    Ok(n)
}

/// One request with DEFER handling; returns the data bytes of the reply.
fn request<H: Hw>(
    hw: &mut H,
    p: u8,
    ver: u8,
    msg: &[u8],
    native: bool,
    out: &mut [u8],
) -> Result<usize> {
    let mut recv = [0u8; 20];
    for _ in 0..32 {
        let n = transfer(hw, p, ver, msg, &mut recv)?;
        match reply(recv[0], native) {
            Reply::Ack => {
                let data = n - 1;
                if data > out.len() {
                    return Err(AuxError::Protocol.into());
                }
                out[..data].copy_from_slice(&recv[1..n]);
                return Ok(data);
            }
            Reply::Nack => return Err(AuxError::Nack.into()),
            Reply::Defer => hw.delay_us(500),
        }
    }
    Err(AuxError::Defer.into())
}

/// Reads DPCD bytes from `addr` into `buf` (any length; 16-byte messages).
pub fn dpcd_read<H: Hw>(hw: &mut H, p: u8, ver: u8, addr: u32, buf: &mut [u8]) -> Result<()> {
    for (i, chunk) in buf.chunks_mut(MAX_PAYLOAD).enumerate() {
        let a = addr + (i * MAX_PAYLOAD) as u32;
        let (h, hl) = header(NATIVE_READ, a, chunk.len());
        let n = request(hw, p, ver, &h[..hl], true, chunk)?;
        if n != chunk.len() {
            return Err(AuxError::Protocol.into());
        }
    }
    Ok(())
}

/// Writes `data` to DPCD from `addr` on.
pub fn dpcd_write<H: Hw>(hw: &mut H, p: u8, ver: u8, addr: u32, data: &[u8]) -> Result<()> {
    for (i, chunk) in data.chunks(MAX_PAYLOAD).enumerate() {
        let a = addr + (i * MAX_PAYLOAD) as u32;
        let (h, _) = header(NATIVE_WRITE, a, chunk.len());
        let mut msg = [0u8; 20];
        msg[..4].copy_from_slice(&h);
        msg[4..4 + chunk.len()].copy_from_slice(chunk);
        request(hw, p, ver, &msg[..4 + chunk.len()], true, &mut [])?;
    }
    Ok(())
}

/// Reads `buf.len()` bytes from I2C device `dev` (7-bit) starting at `offset` (EDID: 0x50),
/// optionally selecting `segment` first (E-DDC segment pointer 0x30).
pub fn i2c_read<H: Hw>(
    hw: &mut H,
    p: u8,
    ver: u8,
    dev: u8,
    segment: Option<u8>,
    offset: u8,
    buf: &mut [u8],
) -> Result<()> {
    if let Some(seg) = segment {
        let (h, _) = header(I2C_WRITE | I2C_MOT, 0x30, 1);
        request(hw, p, ver, &[h[0], h[1], h[2], h[3], seg], false, &mut [])?;
    }
    let (h, _) = header(I2C_WRITE | I2C_MOT, dev as u32, 1);
    request(
        hw,
        p,
        ver,
        &[h[0], h[1], h[2], h[3], offset],
        false,
        &mut [],
    )?;
    for chunk in buf.chunks_mut(MAX_PAYLOAD) {
        let (h, hl) = header(I2C_READ | I2C_MOT, dev as u32, chunk.len());
        let n = request(hw, p, ver, &h[..hl], false, chunk)?;
        if n != chunk.len() {
            return Err(AuxError::Protocol.into());
        }
    }
    let (h, hl) = header(I2C_READ, dev as u32, 0);
    request(hw, p, ver, &h[..hl], false, &mut [])?;
    Ok(())
}

/// Raises the AUX power request of a display-14 port and waits the 600 µs Intel prescribes
/// (the status bit cannot be trusted on this generation).
pub fn power_on<H: Hw>(hw: &mut H, p: u8) {
    if hw.read(port::aux_ctl(p)) & aux::POWER_REQUEST == 0 {
        hw.rmw(port::aux_ctl(p), 0, aux::POWER_REQUEST);
        hw.delay_us(600);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dp::sink::FakeSink;
    use crate::error::Error;
    use crate::sim::Sim;

    #[test]
    fn header_and_reply_codes() {
        assert_eq!(header(NATIVE_READ, 0x00000, 15), ([0x90, 0, 0, 14], 4));
        assert_eq!(header(NATIVE_WRITE, 0x10203, 1), ([0x81, 0x02, 0x03, 0], 4));
        assert_eq!(header(I2C_READ, 0x50, 0).1, 3);
        assert_eq!(reply(0x00, true), Reply::Ack);
        assert_eq!(reply(0x10, true), Reply::Nack);
        assert_eq!(reply(0x20, true), Reply::Defer);
        assert_eq!(reply(0x40, false), Reply::Nack);
        assert_eq!(reply(0x80, false), Reply::Defer);
        assert_eq!(reply(0x00, false), Reply::Ack);
    }

    #[test]
    fn packing() {
        assert_eq!(pack(&[1, 2, 3, 4]), 0x0102_0304);
        assert_eq!(pack(&[0xaa]), 0xaa00_0000);
        let mut b = [0u8; 3];
        unpack(0x1122_3344, &mut b);
        assert_eq!(b, [0x11, 0x22, 0x33]);
    }

    #[test]
    fn ctl_values() {
        let v = ctl_value(4, 14);
        assert_eq!(field(v, aux::SIZE_MASK), 4);
        assert!(v & aux::POWER_REQUEST != 0);
        assert_eq!(v & 0x3ff, (17 << 5) | 31);
        assert_eq!(ctl_value(4, 9) & aux::POWER_REQUEST, 0);
    }

    #[test]
    fn dpcd_round_trip_with_a_sink() {
        let mut s = Sim::new();
        let sink = FakeSink::attach(&mut s, 6);
        let mut caps = [0u8; 16];
        dpcd_read(&mut s, 6, 14, 0, &mut caps).unwrap();
        assert_eq!(caps[0], 0x12);
        assert_eq!(caps[1], 0x0a);
        dpcd_write(&mut s, 6, 14, 0x100, &[0x06, 0x84]).unwrap();
        assert_eq!(sink.borrow().dpcd[0x100], 0x06);
        assert_eq!(sink.borrow().dpcd[0x101], 0x84);
        let mut big = [0u8; 40];
        dpcd_read(&mut s, 6, 14, 0, &mut big).unwrap();
        assert_eq!(big[..16], caps);
    }

    #[test]
    fn defer_then_ack() {
        let mut s = Sim::new();
        let sink = FakeSink::attach(&mut s, 6);
        sink.borrow_mut().defer = 3;
        let mut b = [0u8; 1];
        dpcd_read(&mut s, 6, 14, 0, &mut b).unwrap();
        assert_eq!(b[0], 0x12);
        sink.borrow_mut().defer = 100;
        assert_eq!(
            dpcd_read(&mut s, 6, 14, 0, &mut b),
            Err(Error::Aux(AuxError::Defer))
        );
    }

    #[test]
    fn nack_timeout_and_busy() {
        let mut s = Sim::new();
        let sink = FakeSink::attach(&mut s, 6);
        sink.borrow_mut().nack = true;
        let mut b = [0u8; 1];
        assert_eq!(
            dpcd_read(&mut s, 6, 14, 0, &mut b),
            Err(Error::Aux(AuxError::Nack))
        );
        // no sink on port 5: every attempt times out
        let mut s2 = Sim::new();
        s2.on_write(
            port::aux_ctl(5),
            alloc::boxed::Box::new(|regs, reg, v| {
                let clear = aux::DONE | aux::TIME_OUT_ERROR | aux::RECEIVE_ERROR;
                let next = if v & aux::SEND_BUSY != 0 {
                    (v & !aux::SEND_BUSY & !clear) | aux::TIME_OUT_ERROR
                } else {
                    v & !clear
                };
                regs.insert(reg, next);
            }),
        );
        assert_eq!(
            dpcd_read(&mut s2, 5, 14, 0, &mut b),
            Err(Error::Aux(AuxError::Timeout))
        );
        let mut s3 = Sim::new();
        s3.set(port::aux_ctl(4), aux::SEND_BUSY);
        assert_eq!(
            dpcd_read(&mut s3, 4, 14, 0, &mut b),
            Err(Error::Aux(AuxError::Busy))
        );
        assert_eq!(
            transfer(&mut s3, 4, 14, &[0; 21], &mut [0; 20]),
            Err(Error::Aux(AuxError::TooLong))
        );
    }

    #[test]
    fn edid_over_i2c() {
        let mut s = Sim::new();
        let sink = FakeSink::attach(&mut s, 6);
        let mut e = [0u8; 128];
        i2c_read(&mut s, 6, 14, 0x50, None, 0, &mut e).unwrap();
        assert_eq!(&e[..8], &[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        assert_eq!(e, sink.borrow().edid[..128]);
        let mut ext = [0u8; 16];
        i2c_read(&mut s, 6, 14, 0x50, None, 128, &mut ext).unwrap();
        assert_eq!(ext[0], 0x02);
    }

    #[test]
    fn power_request_once() {
        let mut s = Sim::new();
        power_on(&mut s, 6);
        assert_eq!(s.get(port::aux_ctl(6)), aux::POWER_REQUEST);
        let t = s.now_us();
        assert!(t >= 600);
        power_on(&mut s, 6);
        assert_eq!(s.now_us(), t);
    }
}
