//! Tests of the GuC logic against the simulated register file.

use super::*;
use crate::fw::{parse_guc, test_image};
use crate::prep;
use crate::sim::Sim;
use alloc::boxed::Box;
use alloc::string::ToString;

fn status(bootrom: u32, ukernel: u32, auth: u32) -> u32 {
    prep(GS_BOOTROM, bootrom) | prep(GS_UKERNEL, ukernel) | prep(GS_AUTH_STATUS, auth)
}

/// A GuC whose reset and DMA engine work; after the DMA it reports `after`.
fn guc(after: u32) -> Sim {
    let mut s = Sim::new();
    s.on_write(
        GDRST,
        Box::new(|r, reg, _| {
            r.insert(reg, 0);
            r.insert(GUC_STATUS, GS_MIA_IN_RESET);
        }),
    );
    s.on_write(
        DMA_CTRL,
        Box::new(move |r, reg, v| {
            if v & START_DMA != 0 {
                r.insert(reg, v & 0xffff & !START_DMA);
                r.insert(GUC_STATUS, after);
            } else {
                r.insert(reg, 0);
            }
        }),
    );
    s
}

#[test]
fn decodes_what_linux_logs() {
    // a running GuC as Linux reports it: 0x8002F0EC
    let st = Status::decode(0x8002_f0ec);
    assert_eq!(
        (st.bootrom, st.ukernel, st.auth, st.in_reset),
        (0x76, 0xf0, 2, false)
    );
    assert_eq!(st.load(), Load::Ready);
    assert!(st.authenticated());
    assert_eq!(Status::decode(GS_MIA_IN_RESET).load(), Load::Running);
    assert!(!Status::decode(GS_MIA_IN_RESET).authenticated());
    assert_eq!(Status::decode(status(0x50, 0, 1)).load(), Load::Failed);
    assert_eq!(Status::decode(status(0x76, 0x71, 2)).load(), Load::Failed);
    assert_eq!(Status::decode(u32::MAX).load(), Load::Failed);
    assert!(st.to_string().contains("ready") && st.to_string().contains("good"));
    assert!(Status::decode(status(0x50, 0, 1))
        .to_string()
        .contains("RSA check failed"));
    assert!(Status::decode(GS_MIA_IN_RESET)
        .to_string()
        .contains("core in reset"));
    // read on the Beelink GTi15 Ultra after an upload without ADS: accepted, then stopped
    let st =
        Status::decode(prep(GS_BOOTROM, 0x1a) | prep(GS_UKERNEL, 0x71) | prep(GS_AUTH_STATUS, 2));
    assert!(st.authenticated() && st.load() == Load::Failed);
    assert!(st.to_string().contains("production key found"));
}

#[test]
fn reset_checks_the_core() {
    let mut s = guc(0);
    reset(&mut s).unwrap();
    assert_eq!(s.writes, [(GDRST, GRDOM_GUC)]);
    let mut s = Sim::new();
    s.on_write(
        GDRST,
        Box::new(|r, reg, _| {
            r.insert(reg, 0);
        }),
    );
    assert_eq!(reset(&mut s), Err(Error::GucReset { status: 0 }));
    let mut s = Sim::new();
    assert_eq!(
        reset(&mut s),
        Err(Error::Timeout {
            reg: GDRST,
            last: GRDOM_GUC
        })
    );
}

#[test]
fn upload_sequence_with_a_large_key() {
    let file = test_image(0x1000, 96, (70, 53, 0));
    let img = parse_guc(&file).unwrap();
    let mut s = guc(0x8002_f0ec);
    let params = [7u32; PARAM_WORDS];
    let st = upload(&mut s, &img, &file, 0x0080_0000, 4 << 20, &params).unwrap();
    assert_eq!(st.load(), Load::Ready);
    assert_eq!(s.writes_to(soft_scratch(0)), [0]);
    assert_eq!(s.writes_to(soft_scratch(14)), [7]);
    assert_eq!(s.writes_to(GUC_SHIM_CONTROL), [0x8602]);
    assert_eq!(s.writes_to(GT_PM_CONFIG), [1]);
    // the key is handed over by its address, not through the 64 registers
    assert_eq!(s.writes_to(uos_rsa_scratch(0)), [0x0080_0000 + 0x1080]);
    assert!(s.writes_to(uos_rsa_scratch(1)).is_empty());
    assert_eq!(s.writes_to(DMA_ADDR_0_LOW), [0x0080_0000]);
    assert_eq!(s.writes_to(DMA_ADDR_0_HIGH), [8 << 16]);
    assert_eq!(s.writes_to(DMA_ADDR_1_LOW), [0x2000]);
    assert_eq!(s.writes_to(DMA_ADDR_1_HIGH), [7 << 16]);
    assert_eq!(s.writes_to(DMA_COPY_SIZE), [0x1080]);
    assert_eq!(s.writes_to(DMA_CTRL), [0x0011_0011, 0x0010_0000]);
    // order: parameters, shim, key, source, start
    let pos = |reg| s.writes.iter().position(|(r, _)| *r == reg).unwrap();
    assert!(pos(soft_scratch(1)) < pos(GUC_SHIM_CONTROL));
    assert!(pos(GUC_SHIM_CONTROL) < pos(uos_rsa_scratch(0)));
    assert!(pos(uos_rsa_scratch(0)) < pos(DMA_ADDR_0_LOW));
    assert!(pos(DMA_COPY_SIZE) < pos(DMA_CTRL));
}

#[test]
fn small_keys_go_through_the_registers() {
    let file = test_image(0x1000, 64, (70, 53, 0));
    let img = parse_guc(&file).unwrap();
    let mut s = Sim::new();
    xfer_rsa(&mut s, &img, &file, 0x0080_0000).unwrap();
    assert_eq!(s.writes.len(), 64);
    assert_eq!(
        s.writes[63],
        (
            uos_rsa_scratch(63),
            u32::from_le_bytes([252, 253, 254, 255])
        )
    );
}

#[test]
fn failures_carry_the_status() {
    let file = test_image(0x1000, 96, (70, 53, 0));
    let img = parse_guc(&file).unwrap();
    let p = [0u32; PARAM_WORDS];
    // signature refused
    let bad = status(0x50, 0, 1);
    let mut s = guc(bad);
    assert_eq!(
        upload(&mut s, &img, &file, 0x0080_0000, 4 << 20, &p),
        Err(Error::GucLoad { status: bad })
    );
    // accepted, but no ADS yet
    let no_ads = status(0x76, 0x71, 2);
    let mut s = guc(no_ads);
    let e = upload(&mut s, &img, &file, 0x0080_0000, 4 << 20, &p).unwrap_err();
    assert_eq!(e, Error::GucLoad { status: no_ads });
    assert!(Status::decode(no_ads).authenticated());
    // never ready: ends after the time limit with the last status
    let mut s = guc(status(0x76, 0x10, 2));
    assert!(matches!(
        upload(&mut s, &img, &file, 0x0080_0000, 4 << 20, &p),
        Err(Error::GucLoad { .. })
    ));
    assert!(s.now_us() >= LOAD_TIMEOUT_US as u64);
    // DMA engine does not finish: the flag is taken back
    let mut s = Sim::new();
    assert!(matches!(
        xfer(&mut s, &img, 0x0080_0000),
        Err(Error::Timeout { reg: DMA_CTRL, .. })
    ));
    assert_eq!(s.writes_to(DMA_CTRL).last(), Some(&0x0010_0000));
}

#[test]
fn ggtt_range_of_the_guc() {
    assert!(check_ggtt(4 << 20, 0x1000, 4 << 20).is_ok());
    assert!(check_ggtt(0x3f_f000, 0x1000, 4 << 20).is_err());
    assert!(check_ggtt(GUC_GGTT_TOP - 0x1000, 0x1000, 4 << 20).is_ok());
    assert!(check_ggtt(GUC_GGTT_TOP - 0x1000, 0x1001, 4 << 20).is_err());
    assert!(check_ggtt(u64::MAX, 2, 4 << 20).is_err());
    let file = test_image(0x1000, 96, (70, 53, 0));
    let img = parse_guc(&file).unwrap();
    let mut s = Sim::new();
    assert!(upload(&mut s, &img, &file, 0x1000, 4 << 20, &[0; PARAM_WORDS]).is_err());
    assert!(
        s.writes.is_empty(),
        "a refused upload must not touch the hardware"
    );
}
