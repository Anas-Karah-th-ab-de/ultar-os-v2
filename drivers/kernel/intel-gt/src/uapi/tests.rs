//! Tests of the device queries against the layout of `xe_drm.h`.

use super::*;

fn beelink() -> Device {
    Device {
        devid: 0x7d51,
        revid: 3,
        engines: vec![
            (EngineClass::Render, 0, 0),
            (EngineClass::Copy, 0, 0),
            (EngineClass::Compute, 0, 0),
        ],
        gts: vec![Gt {
            media: false,
            id: 0,
            reference_clock: 19_200_000,
            ip: (12, 74, 4),
        }],
        memory_bytes: 32 << 30,
        memory_used: 4096,
        hwconfig: vec![1, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0],
        guc_submission: (0, 1, 26, 0),
    }
}

fn data(d: &Device, query: u32) -> Vec<u8> {
    let Answer::Size(size) = d.query(query, 0, &[]).unwrap() else {
        panic!("size expected");
    };
    match d.query(query, size, &[]).unwrap() {
        Answer::Data(b) => {
            assert_eq!(b.len() as u32, size);
            b
        }
        other => panic!("{other:?}"),
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u32_at(b, at) as u64 | (u32_at(b, at + 4) as u64) << 32
}

#[test]
fn request_numbers_decode_like_linux_encodes_them() {
    // DRM_IOCTL_XE_DEVICE_QUERY = _IOWR('d', 0x40, 40 bytes)
    let r = Request::decode(0xc028_6440).unwrap();
    assert_eq!((r.nr, r.size, r.read, r.write), (0x40, 40, true, true));
    assert_eq!(r.driver(), Some(XE_DEVICE_QUERY));
    // DRM_IOCTL_VERSION = _IOWR('d', 0x00, 64 bytes): DRM itself, not the driver
    let v = Request::decode(0xc040_6400).unwrap();
    assert_eq!((v.nr, v.driver()), (0, None));
    // a terminal request is not DRM
    assert_eq!(Request::decode(0x5401), None);
}

#[test]
fn engines_are_class_instance_gt() {
    let b = data(&beelink(), QUERY_ENGINES);
    assert_eq!(b.len(), 8 + 3 * 32);
    assert_eq!(u32_at(&b, 0), 3);
    assert_eq!(b[8..14], [0, 0, 0, 0, 0, 0]);
    assert_eq!(b[40..46], [1, 0, 0, 0, 0, 0]);
    assert_eq!(b[72..78], [4, 0, 0, 0, 0, 0]);
}

#[test]
fn memory_is_one_system_region() {
    let b = data(&beelink(), QUERY_MEM_REGIONS);
    assert_eq!(b.len(), 96);
    assert_eq!(u32_at(&b, 0), 1);
    assert_eq!(u32_at(&b, 8), 0, "class system memory, instance 0");
    assert_eq!(u32_at(&b, 12), 4096);
    assert_eq!(u64_at(&b, 16), 32 << 30);
    assert_eq!(u64_at(&b, 24), 4096);
}

#[test]
fn config_has_the_five_values() {
    let b = data(&beelink(), QUERY_CONFIG);
    assert_eq!(b.len(), 48);
    assert_eq!(u32_at(&b, 0), 5);
    assert_eq!(u64_at(&b, 8), 0x0003_7d51);
    assert_eq!(u64_at(&b, 16), 0);
    assert_eq!(u64_at(&b, 24), 4096);
    assert_eq!(u64_at(&b, 32), 48);
    assert_eq!(u64_at(&b, 40), 1);
}

#[test]
fn gt_list_names_clock_memory_and_ip() {
    let b = data(&beelink(), QUERY_GT_LIST);
    assert_eq!(b.len(), 8 + 96);
    assert_eq!(u32_at(&b, 0), 1);
    assert_eq!(u32_at(&b, 8), 0, "main GT on tile 0");
    assert_eq!(u32_at(&b, 20), 19_200_000);
    assert_eq!(u64_at(&b, 24), 1);
    assert_eq!(u64_at(&b, 32), 0);
    assert_eq!(b[40..46], [12, 0, 74, 0, 4, 0]);
}

#[test]
fn hwconfig_is_handed_through() {
    assert_eq!(data(&beelink(), QUERY_HWCONFIG), beelink().hwconfig);
}

#[test]
fn firmware_version_reads_which_one_from_the_buffer() {
    let d = beelink();
    assert_eq!(d.query(QUERY_UC_FW_VERSION, 0, &[]), Ok(Answer::Size(32)));
    let Answer::Data(b) = d.query(QUERY_UC_FW_VERSION, 32, &[0; 32]).unwrap() else {
        panic!("data expected");
    };
    assert_eq!((u32_at(&b, 8), u32_at(&b, 12)), (1, 26));
    let mut huc = [0u8; 32];
    huc[0] = 1;
    assert_eq!(d.query(QUERY_UC_FW_VERSION, 32, &huc), Err(Errno::NoDev));
    huc[0] = 9;
    assert_eq!(d.query(QUERY_UC_FW_VERSION, 32, &huc), Err(Errno::Inval));
    let mut dirty = [0u8; 32];
    dirty[20] = 1;
    assert_eq!(d.query(QUERY_UC_FW_VERSION, 32, &dirty), Err(Errno::Inval));
    assert_eq!(d.query(QUERY_UC_FW_VERSION, 32, &[0; 8]), Err(Errno::Inval));
}

#[test]
fn wrong_sizes_and_unknown_queries_are_refused() {
    let d = beelink();
    assert_eq!(d.query(QUERY_CONFIG, 47, &[]), Err(Errno::Inval));
    assert_eq!(d.query(QUERY_ENGINES, 4096, &[]), Err(Errno::Inval));
    assert_eq!(d.query(5, 0, &[]), Err(Errno::Inval));
    assert_eq!(d.query(99, 0, &[]), Err(Errno::Inval));
}

#[test]
fn the_request_structure_is_parsed_and_checked() {
    let mut arg = [0u8; 40];
    arg[8] = 2; // query
    arg[12] = 48; // size
    arg[16..24].copy_from_slice(&0x7fff_1234_5000u64.to_le_bytes());
    assert_eq!(
        DeviceQuery::parse(&arg),
        Ok(DeviceQuery {
            query: 2,
            size: 48,
            data: 0x7fff_1234_5000
        })
    );
    arg[0] = 1; // extensions are not supported
    assert_eq!(DeviceQuery::parse(&arg), Err(Errno::Inval));
    arg[0] = 0;
    arg[32] = 1; // reserved must be zero
    assert_eq!(DeviceQuery::parse(&arg), Err(Errno::Inval));
    assert_eq!(DeviceQuery::parse(&arg[..39]), Err(Errno::Inval));
}
