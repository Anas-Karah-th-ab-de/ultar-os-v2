//! Tests of the ADS blocks: layout against the firmware interface, content of both blocks.

use super::*;

fn get(blob: &[u8], off: usize) -> u32 {
    word(blob, off)
}

#[test]
fn layout_matches_the_firmware_interface() {
    // sizes of the structures as the xe driver's headers define them
    assert_eq!(ADS_SIZE, 4572);
    assert_eq!(POLICIES_SIZE, 96);
    assert_eq!(SYSTEM_INFO_SIZE, 640);
    assert_eq!(ENGINE_USAGE_SIZE, 16384);
    assert_eq!(UM_INIT_PARAMS_SIZE, 128);
    assert_eq!(STATIC_SIZE, 21820);
    assert_eq!(
        (ADS_SCHEDULER_POLICIES, ADS_GT_SYSTEM_INFO, ADS_PRIVATE_DATA),
        (4100, 4104, 4244)
    );
    assert_eq!(private_data_offset(), 24576);
    assert_eq!(minimal_size(0x4_0000), 24576 + 0x4_0000);
    assert_eq!(minimal_size(1), 24576 + 4096);
}

#[test]
fn minimal_block_content() {
    let base = 0x0120_0000;
    let b = minimal(base, 0x2000, 0x000f_0001).unwrap();
    assert_eq!(b.len(), 24576 + 0x2000);
    assert_eq!(get(&b, 4100), base + 4572);
    assert_eq!(get(&b, 4104), base + 4668);
    assert_eq!(get(&b, 4244), base + 24576);
    assert_eq!(get(&b, 4572 + 64), 500_000);
    assert_eq!(get(&b, 4572 + 68), 1);
    assert_eq!(get(&b, 4572 + 72), 15);
    assert!(b[4668..4668 + 512].iter().all(|v| *v == 32));
    // no engine enabled, 16 doorbells per SQIDI
    assert!(b[4668 + 512..4668 + 576].iter().all(|v| *v == 0));
    assert_eq!(get(&b, 4668 + 576 + 8), 16);
    // everything else stays zero: register lists, golden contexts, capture lists
    assert!(b[..4100].iter().all(|v| *v == 0));
    assert!(b[4108..4244].iter().all(|v| *v == 0));
    assert!(b[24576..].iter().all(|v| *v == 0));
}

#[test]
fn refuses_bad_addresses() {
    assert!(minimal(0x1234, 0, 0).is_err());
    assert!(minimal(0xffff_f000, 0x2000, 0).is_err());
}

#[test]
fn full_block_describes_the_three_engines() {
    let base = 0x0200_0000;
    let b = full(base, 0x2000, 0x000f_0001, &XE_LPG_ENGINES).unwrap();
    // fixed 24576 | register lists 4096 | golden 56K + 8K + 56K | capture 4096 | private
    let (regs, golden) = (24576, 28672);
    let capture = golden + 2 * 57344 + 8192;
    assert_eq!(b.len(), capture + 4096 + 0x2000);
    assert_eq!(full_size(0x2000, &XE_LPG_ENGINES), b.len());
    // enabled masks and mapping of render (0), blitter (3), compute (4)
    let masks: Vec<u32> = (0..6).map(|c| get(&b, 4668 + 512 + c * 4)).collect();
    assert_eq!(masks, [1, 0, 0, 1, 1, 0]);
    assert_eq!((b[4668], b[4668 + 3 * 32], b[4668 + 4 * 32]), (0, 0, 0));
    assert_eq!((b[4668 + 1], b[4668 + 32]), (32, 32));
    // register lists: status page and interrupt mask of each engine, 16 bytes per entry
    assert_eq!((get(&b, 0), get(&b, 4)), (base + regs as u32, 2));
    assert_eq!(
        (get(&b, 3 * 32 * 8), get(&b, 4 * 32 * 8)),
        (base + regs as u32 + 32, base + regs as u32 + 64)
    );
    let list: Vec<u32> = (0..6).map(|i| get(&b, regs + i * 16)).collect();
    assert_eq!(
        list,
        [0x2080, 0x20a8, 0x2_2080, 0x2_20a8, 0x1_a080, 0x1_a0a8]
    );
    // golden contexts: address per class, engine state size without status page and registers
    assert_eq!(get(&b, 4116), base + golden as u32);
    assert_eq!(get(&b, 4116 + 3 * 4), base + golden as u32 + 57344);
    assert_eq!(get(&b, 4116 + 4 * 4), base + golden as u32 + 57344 + 8192);
    assert_eq!(
        (get(&b, 4180), get(&b, 4180 + 12), get(&b, 4180 + 16)),
        (52864, 3712, 52864)
    );
    assert_eq!(get(&b, 4116 + 4), 0);
    // capture lists all point at the empty list; private data follows it
    assert!((0..66).all(|i| get(&b, 4252 + i * 4) == base + capture as u32));
    assert!(b[capture..].iter().all(|v| *v == 0));
    assert_eq!(get(&b, 4244), base + capture as u32 + 4096);
    // the page-fault queues are not used
    assert_eq!(get(&b, 4248), 0);
}

#[test]
fn full_block_refuses_bad_input() {
    assert!(full(0x1234, 0, 0, &XE_LPG_ENGINES).is_err());
    let bad = [Engine {
        class: Class::Render,
        instance: 32,
        mmio_base: 0x2000,
    }];
    assert_eq!(
        full(0x1000, 0, 0, &bad),
        Err(Error::Invalid("engine instance"))
    );
    let many = [XE_LPG_ENGINES[0]; 200];
    assert!(full(0x1000, 0, 0, &many).is_err());
}
