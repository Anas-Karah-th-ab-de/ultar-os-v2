//! SNDRV_PCM-Wiedergabe gegen /dev/snd/pcmC0D<n>p (RDMA-FE). HW_PARAMS/SW_PARAMS/PREPARE/WRITEI/START.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::fd::AsRawFd;

const fn ioc(dir: u64, ty: u8, nr: u64, sz: usize) -> u64 {
    (dir << 30) | ((sz as u64) << 16) | ((ty as u64) << 8) | nr
}
const PCM_HW_PARAMS: u64 = ioc(3, b'A', 0x11, 608);
const PCM_SW_PARAMS: u64 = ioc(3, b'A', 0x13, 136);
const PCM_PREPARE: u64 = ioc(0, b'A', 0x40, 0);
const PCM_START: u64 = ioc(0, b'A', 0x42, 0);
const PCM_DRAIN: u64 = ioc(0, b'A', 0x44, 0);

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

const RATE: u32 = 48000;

fn hw_params() -> [u8; 608] {
    let mut p = [0u8; 608];
    let put = |p: &mut [u8], o: usize, v: u32| p[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut p, 4, 1 << 3); // ACCESS: RW_INTERLEAVED
    put(&mut p, 4 + 32, 1 << 2); // FORMAT: S16_LE
    put(&mut p, 4 + 64, 1); // SUBFORMAT: STD
    let base = 4 + 8 * 32;
    for i in 0..12 {
        put(&mut p, base + i * 12, 0);
        put(&mut p, base + i * 12 + 4, u32::MAX);
        put(&mut p, base + i * 12 + 8, 0);
    }
    let fix = |p: &mut [u8], i: usize, v: u32| {
        put(p, base + i * 12, v);
        put(p, base + i * 12 + 4, v);
        put(p, base + i * 12 + 8, 4);
    };
    fix(&mut p, 0, 16); // SAMPLE_BITS
    fix(&mut p, 1, 32); // FRAME_BITS
    fix(&mut p, 2, 2); // CHANNELS
    fix(&mut p, 3, RATE);
    put(&mut p, base + 5 * 12, 1024); // PERIOD_SIZE min
    put(&mut p, base + 5 * 12 + 4, 4096);
    put(&mut p, base + 12 * 12 + 9 * 12, u32::MAX); // rmask
    p
}

/// Ton/PCM auf pcmC0D<dev>p spielen. `pcm` = 16-bit Stereo LE Interleaved.
pub fn play(dev: u32, pcm: &[u8]) -> Result<(), String> {
    let path = format!("/dev/snd/pcmC0D{dev}p");
    let mut f = OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| format!("{path}: {e}"))?;
    let fd = f.as_raw_fd();
    let mut hp = hw_params();
    if unsafe { ioctl(fd, PCM_HW_PARAMS, hp.as_mut_ptr()) } != 0 {
        return Err(format!(
            "{path}: HW_PARAMS {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { ioctl(fd, PCM_PREPARE) } != 0 {
        return Err(format!(
            "{path}: PREPARE {}",
            std::io::Error::last_os_error()
        ));
    }
    // explizit STARTen (triggert DPCM-BE: UAIF-Startup/hw_params/trigger)
    unsafe { ioctl(fd, PCM_START) };
    f.write_all(pcm).map_err(|e| format!("{path}: write {e}"))?;
    unsafe { ioctl(fd, PCM_DRAIN) };
    Ok(())
}

/// Sinuston erzeugen (16-bit Stereo, 48k).
pub fn tone(hz: f32, ms: u32, vol: f32) -> Vec<u8> {
    let n = (RATE * ms / 1000) as usize;
    let amp = 9000.0 * vol.clamp(0.0, 1.0);
    let fade = 480usize;
    let mut out = Vec::with_capacity(n * 4);
    for i in 0..n {
        let env = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
        let s = (((i as f64) * hz as f64 * std::f64::consts::TAU / RATE as f64).sin() as f32
            * amp
            * env) as i16;
        out.extend_from_slice(&s.to_le_bytes());
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Kernel-Log (abox/uaif) über klogctl lesen – zeigt, ob das BE wirklich getriggert wird.
pub fn klog_audio() -> Vec<String> {
    extern "C" {
        fn klogctl(typ: i32, buf: *mut u8, len: i32) -> i32;
    }
    let mut buf = vec![0u8; 1 << 18];
    let n = unsafe { klogctl(3, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return Vec::new();
    }
    String::from_utf8_lossy(&buf[..n as usize])
        .lines()
        .filter(|l| {
            let d = l.to_lowercase();
            d.contains("abox")
                || d.contains("uaif")
                || d.contains("calliope")
                || d.contains("cod3035")
                || d.contains("rdma")
        })
        .map(|s| s.to_string())
        .collect()
}
