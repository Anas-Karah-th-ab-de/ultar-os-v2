use super::*;
use std::sync::{Arc, Mutex};

/// The JPEG without its DHT segments (as a camera sends it).
fn strip_tables(jpeg: &[u8]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    let mut at = 2;
    while jpeg[at + 1] != 0xDA {
        let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        if jpeg[at + 1] != 0xC4 {
            out.extend_from_slice(&jpeg[at..at + 2 + len]);
        }
        at += 2 + len;
    }
    out.extend_from_slice(&jpeg[at..]);
    out
}

fn sample(w: u16, h: u16) -> Picture {
    let mut rgb = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgb.extend_from_slice(&[
                (u32::from(x) * 255 / u32::from(w)) as u8,
                (u32::from(y) * 255 / u32::from(h)) as u8,
                128,
            ]);
        }
    }
    Picture {
        width: w,
        height: h,
        rgb,
    }
}

#[test]
fn the_standard_tables_have_their_162_codes() {
    for (_, bits, values) in TABLES {
        assert_eq!(
            bits.iter().map(|&b| usize::from(b)).sum::<usize>(),
            values.len()
        );
    }
    assert_eq!(dht().len(), 2 + 2 + 4 * 17 + 12 + 12 + 162 + 162);
}

#[test]
fn a_camera_picture_without_tables_decodes_like_the_original() {
    let jpeg = video::encode(&sample(320, 240)).unwrap();
    let bare = strip_tables(&jpeg);
    assert!(bare.len() < jpeg.len(), "the encoder wrote tables");
    let fixed = with_tables(&bare).unwrap();
    assert_eq!(video::decode(&fixed), video::decode(&jpeg));
    // a JPEG that has its tables stays as it is
    assert_eq!(with_tables(&jpeg).unwrap(), jpeg);
    assert!(with_tables(b"not a jpeg").is_none());
    assert!(with_tables(&[0xFF, 0xD8, 0xFF]).is_none());
}

#[test]
fn larger_camera_pictures_are_scaled_to_the_call_size() {
    let big = video::encode(&sample(640, 480)).unwrap();
    let p = from_camera(&strip_tables(&big)).unwrap();
    assert_eq!((p.width, p.height), video::SIZE);
    assert_eq!(p.rgb.len(), 320 * 240 * 3);
    let same = sample(320, 240);
    assert_eq!(fit(&same, video::SIZE), same);
}

/// Pictures handed on: (own camera, JPEG).
type Shown = Arc<Mutex<Vec<(bool, Vec<u8>)>>>;

struct Collect(Shown);

impl PictureSink for Collect {
    fn show(&mut self, own: bool, jpeg: Vec<u8>) {
        self.0.lock().unwrap().push((own, jpeg));
    }
}

#[test]
fn a_call_reads_the_camera_and_hands_on_the_other_side() {
    let d = tempfile::tempdir().unwrap();
    let node = d.path().join("kamera");
    std::fs::write(
        &node,
        strip_tables(&video::encode(&sample(320, 240)).unwrap()),
    )
    .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut v = CallVideo {
        camera: Some(Camera::open(&node).unwrap()),
        sink: Box::new(Collect(seen.clone())),
    };
    let own = v.capture().unwrap();
    assert_eq!((own.width, own.height), video::SIZE);
    assert!(v.capture().is_none(), "nothing new from the camera");
    v.show(own);
    let got = seen.lock().unwrap().clone();
    assert_eq!(got.len(), 2, "own picture, then the other side's");
    assert!(got[0].0 && !got[1].0);
    assert!(got.iter().all(|(_, j)| video::decode(j).is_some()));
    let mut none = CallVideo {
        camera: None,
        sink: Box::new(Collect(seen)),
    };
    assert!(none.capture().is_none());
    assert!(Camera::open(&d.path().join("missing")).is_err());
}
