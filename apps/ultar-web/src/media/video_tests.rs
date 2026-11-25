use super::*;
use mp4::{AvcConfig, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig};
use openh264::encoder::Encoder;
use openh264::formats::{RgbSliceU8, YUVBuffer};
use std::sync::Arc;

/// NAL units of an Annex B stream.
fn nal_units(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= stream.len() {
        if stream[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &s)| {
            let end = starts.get(n + 1).map_or(stream.len(), |&e| e - 3);
            let mut unit = &stream[s..end];
            while unit.last() == Some(&0) {
                unit = &unit[..unit.len() - 1];
            }
            unit
        })
        .collect()
}

/// An MP4 file with `frames` red-to-blue pictures of 64×48 at 25 per second.
pub(crate) fn mp4_file(frames: u32) -> Vec<u8> {
    let (w, h) = (64usize, 48usize);
    let mut encoder = Encoder::new().unwrap();
    let (mut sps, mut pps, mut samples) = (Vec::new(), Vec::new(), Vec::new());
    for f in 0..frames {
        let shade = (f * 255 / frames.max(1)) as u8;
        let rgb: Vec<u8> = (0..w * h).flat_map(|_| [255 - shade, 0, shade]).collect();
        let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(&rgb, (w, h)));
        let stream = encoder.encode(&yuv).unwrap().to_vec();
        let mut sample = Vec::new();
        for unit in nal_units(&stream) {
            match unit.first().map(|b| b & 0x1F) {
                Some(7) => sps = unit.to_vec(),
                Some(8) => pps = unit.to_vec(),
                Some(_) => {
                    sample.extend_from_slice(&(unit.len() as u32).to_be_bytes());
                    sample.extend_from_slice(unit);
                }
                None => {}
            }
        }
        samples.push(sample);
    }
    let config = Mp4Config {
        major_brand: str::parse("isom").unwrap(),
        minor_version: 512,
        compatible_brands: vec![str::parse("isom").unwrap(), str::parse("avc1").unwrap()],
        timescale: 1000,
    };
    let mut writer = Mp4Writer::write_start(Cursor::new(Vec::new()), &config).unwrap();
    writer
        .add_track(&TrackConfig {
            track_type: TrackType::Video,
            timescale: 25,
            language: "und".into(),
            media_conf: MediaConfig::AvcConfig(AvcConfig {
                width: w as u16,
                height: h as u16,
                seq_param_set: sps,
                pic_param_set: pps,
            }),
        })
        .unwrap();
    for (i, s) in samples.into_iter().enumerate() {
        let sample = Mp4Sample {
            start_time: i as u64,
            duration: 1,
            rendering_offset: 0,
            is_sync: i == 0,
            bytes: s.into(),
        };
        writer.write_sample(1, &sample).unwrap();
    }
    writer.write_end().unwrap();
    writer.into_writer().into_inner()
}

#[test]
fn an_h264_mp4_gives_its_pictures_in_time() {
    let mut v = VideoStream::open(Bytes(Arc::new(mp4_file(10)))).unwrap();
    assert_eq!(v.size, (64, 48));
    let mut shown = Vec::new();
    while let Some((at, p)) = v.next_picture() {
        assert_eq!((p.width, p.height, p.xrgb.len()), (64, 48, 64 * 48 * 4));
        shown.push((at, p));
    }
    assert!(shown.len() >= 9, "{}", shown.len());
    assert_eq!(shown[1].0, Duration::from_millis(40));
    // the first picture is red, the last one mostly blue (bytes: blue, green, red, x)
    let first = &shown[0].1.xrgb;
    assert!(first[2] > 200 && first[0] < 60, "{:?}", &first[..4]);
    let last = &shown.last().unwrap().1.xrgb;
    assert!(last[0] > last[2], "{:?}", &last[..4]);
}

#[test]
fn not_an_mp4() {
    assert_eq!(
        VideoStream::open(Bytes(Arc::new(b"not a video".to_vec()))).err(),
        Some(VideoError::NoVideo)
    );
    assert_eq!(to_annex_b(&[0, 0, 0, 2, 9, 9], 4), [0, 0, 0, 1, 9, 9]);
}
