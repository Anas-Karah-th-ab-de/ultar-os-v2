//! The GStreamer audio sink of the `media` feature: an element registered in the running host
//! with a higher rank than the sinks of the distributions, so Servo's pipeline (servo-media asks
//! for `autoaudiosink`) picks it. Every buffer it is given is written to a stream of the Ultar
//! sound server ([`crate::pcm`]: 48 kHz, 16 bit, stereo, non-blocking); when it is busy the rest of the
//! buffer is dropped (no backlog to grow). Its pad accepts exactly that format, so the
//! converters in front of it (`audioconvert`, `audioresample`) do the rest.

use crate::pcm::{self, Sink};
use gst::glib;
use gst::prelude::*;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::*;
use gstreamer as gst;
use gstreamer_base as gst_base;
use std::sync::{LazyLock, Mutex};

/// Above `Rank::PRIMARY` (256), where the sinks of sound servers and ALSA sit.
const RANK: i32 = 256 + 100;

mod imp {
    use super::*;

    /// The instance state: where the card is, and the card, opened on first use (`Silent` when
    /// there is none: the sound is dropped and the pictures still follow the clock).
    pub struct UltarPcmSink {
        location: Mutex<String>,
        card: Mutex<Option<Sink>>,
    }

    impl Default for UltarPcmSink {
        fn default() -> Self {
            UltarPcmSink {
                location: Mutex::new(pcm::socket()),
                card: Mutex::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UltarPcmSink {
        const NAME: &'static str = "UltarPcmSink";
        type Type = super::UltarPcmSink;
        type ParentType = gst_base::BaseSink;
    }

    impl ObjectImpl for UltarPcmSink {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
                vec![glib::ParamSpecString::builder("location")
                    .nick("Location")
                    .blurb("The sound server's stream socket")
                    .default_value(Some(pcm::AUDIO_SOCKET))
                    .build()]
            });
            PROPS.as_ref()
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            if pspec.name() == "location" {
                if let Ok(Some(path)) = value.get::<Option<String>>() {
                    *self.location.lock().unwrap_or_else(|p| p.into_inner()) = path;
                    // the next buffer opens the new place
                    *self.card.lock().unwrap_or_else(|p| p.into_inner()) = None;
                }
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "location" => self
                    .location
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .to_value(),
                _ => String::new().to_value(),
            }
        }
    }

    impl GstObjectImpl for UltarPcmSink {}

    impl ElementImpl for UltarPcmSink {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new(
                    "Ultar PCM Sink",
                    "Sink/Audio",
                    "Writes raw 48 kHz 16-bit stereo audio to the Ultar sound server",
                    "Ultar",
                )
            });
            Some(&*META)
        }

        fn pad_templates() -> &'static [gst::PadTemplate] {
            static TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
                let caps = gst::Caps::builder("audio/x-raw")
                    .field("format", "S16LE")
                    .field("rate", pcm::RATE as i32)
                    .field("channels", pcm::CHANNELS as i32)
                    .field("layout", "interleaved")
                    .build();
                gst::PadTemplate::new(
                    "sink",
                    gst::PadDirection::Sink,
                    gst::PadPresence::Always,
                    &caps,
                )
                .map(|t| vec![t])
                .unwrap_or_default()
            });
            TEMPLATES.as_ref()
        }
    }

    impl BaseSinkImpl for UltarPcmSink {
        fn stop(&self) -> Result<(), gst::ErrorMessage> {
            // give the card back
            *self.card.lock().unwrap_or_else(|p| p.into_inner()) = None;
            Ok(())
        }

        fn render(&self, buffer: &gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
            // the card is shared across the sink's streaming threads
            let mut slot = self.card.lock().unwrap_or_else(|p| p.into_inner());
            if slot.is_none() {
                let at = self
                    .location
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                *slot = Some(Sink::open(std::path::Path::new(&at)));
            }
            let Some(card) = slot.as_mut() else {
                return Ok(gst::FlowSuccess::Ok);
            };
            if !card.audible() {
                return Ok(gst::FlowSuccess::Ok);
            }
            let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
            let bytes: &[u8] = &map;
            let mut done = 0;
            while done < bytes.len() {
                let taken = card.offer(&bytes[done..]);
                if taken == 0 {
                    break; // the card is busy: drop the rest, stay ahead of the clock
                }
                done += taken;
            }
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    /// The sink element.
    pub struct UltarPcmSink(ObjectSubclass<imp::UltarPcmSink>)
        @extends gst_base::BaseSink, gst::Element, gst::Object;
}

/// Starts GStreamer and registers the sink so `autoaudiosink` (and with it Servo's pipeline)
/// considers it first.
pub fn install() {
    if let Err(e) = gst::init() {
        eprintln!("ultar-web-engine: GStreamer did not start: {e}");
        return;
    }
    let registered = gst::Element::register(
        None,
        "ultarpcmsink",
        gst::Rank::from(RANK),
        UltarPcmSink::static_type(),
    );
    if let Err(e) = registered {
        eprintln!("ultar-web-engine: audio sink not registered: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(bytes: &[u8]) -> gst::Buffer {
        gst::Buffer::from_slice(bytes.to_vec())
    }

    #[test]
    fn the_sink_is_registered_above_the_distribution_sinks_and_takes_only_the_card_format() {
        install();
        let factory = gst::ElementFactory::find("ultarpcmsink");
        assert!(factory.is_some());
        let rank: i32 = factory.map(|f| f.rank().into()).unwrap_or(0);
        assert!(rank > 256 + 10, "rank {rank}");
        let el = gst::ElementFactory::make("ultarpcmsink").build();
        assert!(el.is_ok());
        let el = el.unwrap();
        let caps = el.static_pad("sink").map(|p| p.pad_template_caps());
        let s = caps.as_ref().and_then(|c| c.structure(0)).unwrap();
        assert_eq!(s.name(), "audio/x-raw");
        assert_eq!(s.get::<i32>("rate").unwrap(), 48_000);
        assert_eq!(s.get::<i32>("channels").unwrap(), 2);
        assert_eq!(s.get::<&str>("format").unwrap(), "S16LE");
    }

    #[test]
    fn buffers_reach_the_sound_server_unchanged_and_in_order() {
        install();
        let dir = std::env::temp_dir().join(format!("ultar-pcm-sink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let node = dir.join("pcm");
        std::fs::write(&node, b"").unwrap();
        let socket = dir.join("server.sock");
        crate::pcm::stand_in::serve(&socket, &node);
        let el = gst::ElementFactory::make("ultarpcmsink")
            .property("location", socket.to_str().unwrap())
            .property("sync", false)
            .build()
            .unwrap();
        let pad = el.static_pad("sink").unwrap();
        el.set_state(gst::State::Playing).unwrap();
        let caps = gst::Caps::builder("audio/x-raw")
            .field("format", "S16LE")
            .field("rate", 48_000i32)
            .field("channels", 2i32)
            .field("layout", "interleaved")
            .build();
        assert!(pad.send_event(gst::event::StreamStart::new("ultar-test")));
        assert!(pad.send_event(gst::event::Caps::new(&caps)));
        assert!(
            pad.send_event(gst::event::Segment::new(&gst::FormattedSegment::<
                gst::ClockTime,
            >::new()))
        );
        let a: Vec<u8> = (0..64u8).collect();
        let b: Vec<u8> = (64..128u8).collect();
        assert!(pad.chain(buffer(&a)).is_ok());
        assert!(pad.chain(buffer(&b)).is_ok());
        el.set_state(gst::State::Null).unwrap();
        crate::pcm::stand_in::wait_for(&node, 128);
        let written = std::fs::read(&node).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(written, (0..128u8).collect::<Vec<u8>>());
    }

    #[test]
    fn without_a_card_the_buffers_are_dropped_and_the_stream_goes_on() {
        install();
        let el = gst::ElementFactory::make("ultarpcmsink")
            .property("location", "/nonexistent/ultar-pcm")
            .property("sync", false)
            .build()
            .unwrap();
        let pad = el.static_pad("sink").unwrap();
        el.set_state(gst::State::Playing).unwrap();
        assert!(pad.send_event(gst::event::StreamStart::new("ultar-test2")));
        let caps = gst::Caps::builder("audio/x-raw")
            .field("format", "S16LE")
            .field("rate", 48_000i32)
            .field("channels", 2i32)
            .field("layout", "interleaved")
            .build();
        assert!(pad.send_event(gst::event::Caps::new(&caps)));
        assert!(
            pad.send_event(gst::event::Segment::new(&gst::FormattedSegment::<
                gst::ClockTime,
            >::new()))
        );
        assert!(pad.chain(buffer(&[0u8; 256])).is_ok());
        el.set_state(gst::State::Null).unwrap();
    }
}
