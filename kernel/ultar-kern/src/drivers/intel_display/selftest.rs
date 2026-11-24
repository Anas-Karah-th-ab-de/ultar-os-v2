//! INTELTEST – the driver's self test on real hardware (bare metal or the iGPU passed to a VM):
//! takeover readout, a mode set when nothing runs (stage B), vertical blanks (rate within ±1 %
//! measured with the display's own time base), flips to kernel buffers with pipe CRC proof
//! (pattern A, B, A: A≠B, A=A), hot-plug status and EDID of the active output. Ends with one line
//! `INTELTEST: ALLES OK` or `INTELTEST: FEHLER <list>`.

use super::buffer::ScanBuf;
use super::device::{Device, Output};
use super::DEVICE;
use crate::kprintln;
use alloc::string::String;
use alloc::vec::Vec;
use ultar_intel_display::mode::Timing;
use ultar_intel_display::{crc, edid, regs, vblank, Hw};

const COLORS: [u32; 8] = [
    0xffffff, 0xffff00, 0x00ffff, 0x00ff00, 0xff00ff, 0xff0000, 0x0000ff, 0x000000,
];

fn pattern_a(x: u32, _y: u32, w: u32) -> u32 {
    COLORS[(x * 8 / w.max(1)) as usize % 8]
}

fn pattern_b(x: u32, y: u32) -> u32 {
    if (x / 64 + y / 64) % 2 == 0 {
        0x20_40_80
    } else {
        0xe0_c0_10
    }
}

/// Failed checks, collected.
struct Report {
    fails: Vec<&'static str>,
}

impl Report {
    fn check(&mut self, ok: bool, what: &'static str) {
        kprintln!("INTELTEST: {} {}", if ok { "ok  " } else { "FAIL" }, what);
        if !ok {
            self.fails.push(what);
        }
    }
}

/// Waits for the frame counter of `p` to move past `after` (tight poll, at most `limit_ns`);
/// returns count and time.
fn frame_edge(dev: &mut Device, p: u8, after: u32, limit_ns: u64) -> Option<(u32, u64)> {
    let reg = regs::pipe(p, regs::FRMCOUNT);
    let start = crate::arch::uptime_ns();
    loop {
        let c = dev.mmio.read(reg);
        let now = crate::arch::uptime_ns();
        if c.wrapping_sub(after) as i32 > 0 {
            return Some((c, now));
        }
        if now - start > limit_ns {
            return None;
        }
    }
}

/// Measures the refresh of pipe `p` over 120 frames: CPU time between two counter edges (the
/// verdict) and the display time base (`PIPE_FRMTMSTMP` delta at the measured base rate, printed).
fn measure_rate(dev: &mut Device, p: u8) -> Option<u64> {
    let now = dev.mmio.read(regs::pipe(p, regs::FRMCOUNT));
    // the display time base runs now: measure its rate if that was not possible at takeover
    let _ = dev.vblank(p);
    let Some((c0, t0)) = frame_edge(dev, p, now, 200_000_000) else {
        kprintln!("INTELTEST: frame counter stands at {}", now);
        return None;
    };
    let a = vblank::read_raw(&mut dev.mmio, p);
    let Some((c1, t1)) = frame_edge(dev, p, c0.wrapping_add(119), 4_000_000_000) else {
        kprintln!("INTELTEST: frame counter stopped after {}", c0);
        return None;
    };
    let b = vblank::read_raw(&mut dev.mmio, p);
    let frames = c1.wrapping_sub(c0) as u64;
    let cpu_mhz = frames * 1_000_000_000_000 / (t1 - t0).max(1);
    let ts = vblank::rate_from_ticks(
        b.frame.wrapping_sub(a.frame) as u64,
        b.frame_ts.wrapping_sub(a.frame_ts) as u64,
        dev.boot.ts_khz,
    )
    .unwrap_or(0);
    kprintln!(
        "INTELTEST: {} frames in {} us: {} mHz (CPU clock); display time base {} kHz: {} mHz",
        frames,
        (t1 - t0) / 1000,
        cpu_mhz,
        dev.boot.ts_khz,
        ts
    );
    Some(cpu_mhz)
}

/// Flips `p` to `surf`, waits for the latch and returns 4 stable frame CRCs.
fn show_and_crc(
    dev: &mut Device,
    p: u8,
    surf: u32,
) -> Result<Vec<u32>, ultar_intel_display::Error> {
    dev.flip(p, surf)?;
    for _ in 0..10 {
        dev.wait_vblank(p, 100_000)?;
        if dev.flip_done(p).is_some() {
            break;
        }
    }
    crc::collect(&mut dev.mmio, p, 2, 4, 100_000)
}

/// Picks the mode for a mode set: the EDID's preferred mode if the link and clock allow it,
/// else 1920x1080@60 (CEA timing).
fn pick_mode(edid_img: Option<&[u8]>) -> Timing {
    let fhd = Timing {
        clock: 148_500,
        hdisplay: 1920,
        hsync_start: 2008,
        hsync_end: 2052,
        htotal: 2200,
        vdisplay: 1080,
        vsync_start: 1084,
        vsync_end: 1089,
        vtotal: 1125,
        flags: 5,
    };
    edid_img
        .and_then(|e| edid::parse(e).ok())
        .and_then(|i| i.timings.first().copied())
        .filter(|t| t.validate().is_ok() && t.clock <= 300_000)
        .unwrap_or(fhd)
}

/// Stage B: light the first connected DP output on pipe A.
fn modeset_first(dev: &mut Device, r: &mut Report) -> Option<u8> {
    let outs: Vec<Output> = dev.outputs.clone();
    for o in outs.into_iter().filter(|o| o.dp) {
        let pr = dev.probe(o);
        kprintln!(
            "INTELTEST: port {} connected={} dpcd={:?} edid={}",
            o.port,
            pr.connected,
            pr.caps,
            pr.edid.as_ref().map_or(0, |e| e.len())
        );
        let Some(caps) = pr.caps else { continue };
        let t = pick_mode(pr.edid.as_deref());
        let mut buf = match dev.new_buffer(t.hdisplay as u32, t.vdisplay as u32) {
            Ok(b) => b,
            Err(e) => {
                kprintln!("INTELTEST: buffer: {e}");
                r.check(false, "buffer for the mode set");
                return None;
            }
        };
        let w = buf.width;
        buf.draw(|x, y| pattern_a(x, y, w));
        let res = dev.modeset_dp(0, o, t, &buf, caps);
        if let Err(e) = res {
            kprintln!("INTELTEST: mode set on port {}: {e}", o.port);
            dev.keep(buf); // freed unless the plane still shows it
            r.check(false, "mode set (stage B)");
            return None;
        }
        r.check(true, "mode set (stage B)");
        let wells = dev.mmio.read(ultar_intel_display::power::PWR_WELL_CTL2);
        let cd = ultar_intel_display::cdclk::read(&mut dev.mmio, 14);
        kprintln!(
            "INTELTEST: after the mode set: wells {:#x}, CDCLK {:?} kHz",
            wells,
            cd
        );
        let dbuf: Vec<u32> = ultar_intel_display::power::DBUF_CTL
            .iter()
            .map(|r| dev.mmio.read(*r))
            .collect();
        let pm = (
            dev.mmio.read(ultar_intel_display::clocks::PMDEMAND0),
            dev.mmio.read(ultar_intel_display::clocks::PMDEMAND1),
        );
        let mem = dev.mmio.read(ultar_intel_display::init::MEM_SS_INFO);
        kprintln!("INTELTEST: DBUF S1..S4 {:08x?}, PM demand {:08x} {:08x}, memory info {:#x}, DDB {:#x}, WM0 {:#x}", dbuf, pm.0, pm.1, mem, dev.mmio.read(0x7027c), dev.mmio.read(0x70240));
        dev.pipes[0].fw_surf = Some(buf.gtt);
        dev.keep(buf); // stays on the screen
        return Some(0);
    }
    kprintln!("INTELTEST: no DP output with a sink answering on AUX");
    None
}

/// Two buffers of `w`×`h`, or none (a single one is freed again).
fn two_buffers(dev: &mut Device, w: u32, h: u32) -> Option<(ScanBuf, ScanBuf)> {
    match (dev.new_buffer(w, h), dev.new_buffer(w, h)) {
        (Ok(a), Ok(b)) => Some((a, b)),
        (a, b) => {
            for x in [a, b].into_iter().flatten() {
                dev.free_buffer(x);
            }
            None
        }
    }
}

/// Real scan-out: the plane shows what was flipped last and the pipe saw no FIFO underrun.
fn check_scanout(dev: &mut Device, p: u8, r: &mut Report) {
    let iir = dev.mmio.read(regs::irq::pipe_iir(p));
    let live = dev.mmio.read(regs::plane(p, 0, regs::PLANE_SURFLIVE)) & !0xfff;
    let want = dev.pipes[p as usize].plane.map_or(0, |pl| pl.surf);
    kprintln!(
        "INTELTEST: PLANE_SURFLIVE {:#x} (expected {:#x}), pipe IIR {:#x}",
        live,
        want,
        iir
    );
    r.check(live == want, "plane scans out the surface flipped last");
    r.check(iir & regs::irq::PIPE_FIFO_UNDERRUN == 0, "no FIFO underrun");
}

type Crcs = Result<Vec<u32>, ultar_intel_display::Error>;

/// Pattern A, B, A: A != B and A == A again.
fn check_crcs(r: &mut Report, ra: Crcs, rb: Crcs, ra2: Crcs) {
    const WHAT: &str = "pipe CRC: pattern A != B and A == A again";
    match (ra, rb, ra2) {
        (Ok(x), Ok(y), Ok(z)) => {
            kprintln!("INTELTEST: CRC A {:08x?} B {:08x?} A {:08x?}", x, y, z);
            let v = crc::Verdict::from_runs(&x, &y, &z);
            r.check(v.is_ok_and(|v| v.ok()), WHAT);
        }
        (x, y, z) => {
            kprintln!(
                "INTELTEST: CRC runs: {:?} {:?} {:?}",
                x.err(),
                y.err(),
                z.err()
            );
            r.check(false, WHAT);
        }
    }
}

fn crc_check(dev: &mut Device, p: u8, r: &mut Report) {
    let Some(pl) = dev.pipes[p as usize].plane else {
        r.check(false, "primary plane present");
        return;
    };
    let (w, h) = (pl.width, pl.height);
    let Some((mut a, mut b)) = two_buffers(dev, w, h) else {
        r.check(false, "two scan-out buffers");
        return;
    };
    a.draw(|x, y| pattern_a(x, y, w));
    b.draw(pattern_b);
    crc::start(&mut dev.mmio, p, crc::SOURCE_PIPE);
    // the picture shown before the test, to compare the flip back with
    let before = crc::collect(&mut dev.mmio, p, 2, 4, 100_000);
    let ra = show_and_crc(dev, p, a.gtt);
    let rb = show_and_crc(dev, p, b.gtt);
    let ra2 = show_and_crc(dev, p, a.gtt);
    let fw = dev.pipes[p as usize].fw_surf;
    let back = fw.map(|s| show_and_crc(dev, p, s));
    crc::stop(&mut dev.mmio, p);
    check_scanout(dev, p, r);
    check_crcs(r, ra, rb, ra2);
    if let Some(b2) = back {
        kprintln!(
            "INTELTEST: CRC before the test {:08x?}, after the flip back {:08x?}",
            before,
            b2
        );
        let same =
            matches!((&before, &b2), (Ok(x), Ok(y)) if x.last().is_some() && x.last() == y.last());
        r.check(
            same,
            "flip back shows the picture from before the test (same CRC)",
        );
    }
    // whatever is not on the screen any more is freed
    dev.keep(a);
    dev.keep(b);
}

/// Prints what the takeover found.
fn print_boot(dev: &Device) {
    kprintln!("INTELTEST: {} display {} at {:02x}:{:02x}.{}, wells {:#x}, CDCLK {:?} kHz, DC state was {:#x}", dev.plat.name, dev.plat.display_ver, dev.pci.0, dev.pci.1, dev.pci.2, dev.boot.wells, dev.boot.cdclk_khz, dev.boot.dc_state_was);
    let from = if dev.outputs_from_vbt {
        "VBT"
    } else {
        "no VBT"
    };
    kprintln!("INTELTEST: outputs ({}): {:?}", from, dev.outputs);
    for (p, e) in &dev.boot.skipped {
        kprintln!("INTELTEST: firmware pipe {p} left alone: {e}");
    }
    for a in &dev.boot.found {
        kprintln!(
            "INTELTEST: firmware pipe {} port {} {:?} {} bpc x{} {:?} plane {:?} {:?}",
            a.pipe,
            a.port,
            a.mode,
            a.bpc,
            a.lanes,
            a.timing,
            a.plane.map(|p| p.cfg),
            a.plane_error
        );
    }
}

/// Hot-plug live status and EDID of the output pipe `p` drives.
fn check_output(dev: &mut Device, p: u8, r: &mut Report) {
    let port = dev.pipes[p as usize].port;
    let Some(o) = dev.outputs.iter().copied().find(|o| o.port == port) else {
        kprintln!("INTELTEST: active port {} is not among the outputs", port);
        return;
    };
    let pr = dev.probe(o);
    r.check(pr.connected, "hot-plug live status shows the active output");
    let e = pr.edid.as_deref().map(edid::parse);
    if let Some(Ok(i)) = &e {
        kprintln!(
            "INTELTEST: EDID {} {:?} {} modes, preferred {:?}",
            core::str::from_utf8(&i.vendor).unwrap_or("?"),
            i.name,
            i.timings.len(),
            i.timings.first()
        );
    }
    r.check(matches!(e, Some(Ok(_))), "EDID of the active output");
}

fn run_on(dev: &mut Device, r: &mut Report) {
    print_boot(dev);
    let mut p = dev.pipes.iter().position(|x| x.active).map(|i| i as u8);
    if p.is_none() && dev.plat.can_modeset() {
        p = modeset_first(dev, r);
    }
    let Some(p) = p else {
        r.check(false, "an active pipe (firmware or mode set)");
        return;
    };
    let expected = match dev.pipes[p as usize].timing.refresh_mhz() {
        0 => 60_000,
        m => m as u64,
    };
    match measure_rate(dev, p) {
        Some(mhz) => r.check(
            vblank::within(mhz, expected, 10),
            "vblank rate within 1 % of the mode",
        ),
        None => r.check(false, "vblank counter runs"),
    }
    crc_check(dev, p, r);
    check_output(dev, p, r);
    let changed = dev.poll_hotplug();
    kprintln!("INTELTEST: hot-plug changes since takeover: {:#x}", changed);
}

/// Runs the test if the driver bound a device; prints the verdict line.
pub fn run() {
    let mut guard = DEVICE.lock();
    let Some(dev) = guard.as_mut() else {
        kprintln!("INTELTEST: no Intel display device bound");
        return;
    };
    let mut r = Report { fails: Vec::new() };
    run_on(dev, &mut r);
    if r.fails.is_empty() {
        kprintln!("INTELTEST: ALLES OK");
    } else {
        let list: Vec<String> = r.fails.iter().map(|s| String::from(*s)).collect();
        kprintln!("INTELTEST: FEHLER ({})", list.join("; "));
    }
}
