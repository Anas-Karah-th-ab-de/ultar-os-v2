use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ultar-accountd-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample() -> Stored {
    let mut s = Stored::default();
    s.users.push(UserRecord {
        name: "anas".into(),
        uid: 1000,
        display: "أنس قرة محمد".into(),
        kind: UserKind::Administrator,
        picture: PictureInfo::Custom(2),
        auto_login: false,
    });
    s.users.push(UserRecord {
        name: "leila".into(),
        uid: 1001,
        display: "Leila".into(),
        kind: UserKind::Standard,
        picture: PictureInfo::Builtin(4),
        auto_login: true,
    });
    s.retired.insert(1002);
    s.pictures = 2;
    s
}

#[test]
fn accounts_survive_a_round_trip_through_the_disk() {
    let dir = temp("round");
    assert_eq!(load(&dir).unwrap(), Stored::default());
    save(&dir, &sample()).unwrap();
    assert_eq!(load(&dir).unwrap(), sample());
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    save_picture(&dir, "anas", b"\x89PNG").unwrap();
    assert_eq!(load_picture(&dir, "anas").unwrap(), b"\x89PNG");
    remove_picture(&dir, "anas");
    assert!(load_picture(&dir, "anas").is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_cut_and_every_changed_byte_is_detected() {
    let dir = temp("cut");
    save(&dir, &sample()).unwrap();
    let good = fs::read(dir.join(FILE)).unwrap();
    for len in 0..good.len() {
        fs::write(dir.join(FILE), &good[..len]).unwrap();
        assert!(load(&dir).is_err(), "cut at {len}");
    }
    let mut x: u32 = 0x1234_5678;
    for at in 0..good.len() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mut bad = good.clone();
        bad[at] ^= (x as u8) | 1;
        fs::write(dir.join(FILE), &bad).unwrap();
        assert!(load(&dir).is_err(), "byte {at}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn inconsistent_bodies_are_damage() {
    let body = encode(&sample());
    for bad in [
        body.replace("kind=admin", "kind=root"),
        body.replace("user leila", "user anas"),
        body.replace("uid=1001", "uid=1000"),
        body.replace("auto=0", "auto=1"),
        body.replace("auto=1", "auto=2"),
        body.replace("picture=builtin:4", "picture=web:4"),
        body.replace("retired 1002", "retired x"),
        format!("{body}group x\n"),
        body.replace("display=", "display=zz"),
    ] {
        assert_eq!(decode(&bad), Err(StoreError::Damaged), "{bad}");
    }
    let many: String = (0..=MAX_LOCAL_USERS)
        .map(|i| {
            format!(
                "user u{i} uid={} kind=standard display=41 picture=initials auto=0\n",
                1000 + i
            )
        })
        .collect();
    assert_eq!(decode(&many), Err(StoreError::Damaged));
    assert!(!StoreError::Io.to_string().is_empty());
}
