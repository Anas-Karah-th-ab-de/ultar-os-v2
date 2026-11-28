use super::*;

const PASS: &[u8] = b"richtig-langes-passwort";

#[test]
fn format_then_open() {
    let f = format(PASS, &Params::TEST).unwrap();
    assert!(is_luks(&f.metadata));
    assert_eq!(f.metadata.len() as u64, KEYSLOTS_OFFSET + 258_048);
    let o = open_bytes(&f.metadata, PASS).unwrap();
    assert_eq!(&o.key[..], &f.key[..]);
    assert_eq!(o.offset, DATA_OFFSET);
}

#[test]
fn wrong_passphrase_is_refused() {
    let f = format(PASS, &Params::TEST).unwrap();
    assert_eq!(
        open_bytes(&f.metadata, b"falsch").err(),
        Some(Error::Passphrase)
    );
}

#[test]
fn damaged_primary_falls_back_to_secondary() {
    let mut f = format(PASS, &Params::TEST).unwrap();
    f.metadata[5000] ^= 0xff;
    assert!(open_bytes(&f.metadata, PASS).is_ok());
    f.metadata[HDR_SIZE as usize + 5000] ^= 0xff;
    assert!(matches!(
        open_bytes(&f.metadata, PASS),
        Err(Error::Header(_))
    ));
}

#[test]
fn not_luks() {
    assert_eq!(
        open_bytes(&vec![0u8; 1 << 16], PASS).err(),
        Some(Error::NotLuks)
    );
    assert!(!is_luks(&[0u8; 8]));
}

#[test]
fn damaged_keyslot_area_fails_closed() {
    let mut f = format(PASS, &Params::TEST).unwrap();
    f.metadata[KEYSLOTS_OFFSET as usize + 100] ^= 1;
    assert_eq!(open_bytes(&f.metadata, PASS).err(), Some(Error::Passphrase));
}

#[test]
fn xts_round_trip_depends_on_sector() {
    let key = [7u8; KEY_LEN];
    let s = Sectors::new(&key);
    let plain: Vec<u8> = (0..1024u32).map(|i| i as u8).collect();
    let mut a = plain.clone();
    s.encrypt(5, &mut a);
    assert_ne!(a, plain);
    let mut b = plain.clone();
    s.encrypt(6, &mut b);
    assert_ne!(a[..512], b[..512]);
    // sector 6 of the first run is sector 6 of the second
    assert_eq!(a[512..], b[..512]);
    s.decrypt(5, &mut a);
    assert_eq!(a, plain);
}

#[test]
fn start_info_travels_in_a_token() {
    let info = StartInfo {
        lang: "de".into(),
        keyboard: "ar+us".into(),
    };
    let f = format_with(PASS, &Params::TEST, Some(&info)).unwrap();
    let mut read = |at: u64, buf: &mut [u8]| {
        let at = at as usize;
        buf.copy_from_slice(
            f.metadata
                .get(at..at + buf.len())
                .ok_or(Error::Header("short"))?,
        );
        Ok(())
    };
    assert_eq!(start_info(&mut read).unwrap(), Some(info));
    assert!(
        open_bytes(&f.metadata, PASS).is_ok(),
        "the token does not disturb opening"
    );
    let plain = format(PASS, &Params::TEST).unwrap();
    let mut read = |at: u64, buf: &mut [u8]| {
        let at = at as usize;
        buf.copy_from_slice(
            plain
                .metadata
                .get(at..at + buf.len())
                .ok_or(Error::Header("short"))?,
        );
        Ok(())
    };
    assert_eq!(start_info(&mut read).unwrap(), None);
}

#[test]
fn af_split_merge() {
    let key = [3u8; 64];
    let s = af::split(&key, 10).unwrap();
    assert_eq!(&af::merge(&s, 64, 10)[..], &key[..]);
}
