//! The GStreamer audio sink of the `media` feature: an element registered in the running host
//! with the highest rank, so Servo's pipeline (servo-media) picks it like `autoaudiosink` would.
//! Every buffer it is given is written to the Ultar sound card (`crate::pcm::Sink`); when the
//! card is busy the rest of the buffer is dropped (non-blocking, no backlog to grow).

use std::sync::Mutex;

mod imp {
    use crate::pcm::Sink;
    use gstreamer::prelude::*;
    use gstreamer::subclass::prelude::*;
    use gstreamer_base::subclass::prelude::*;
    use std::sync::Mutex;

    /// The instance state: the card, opened on first use; `None` when there is none (sound is
    /// dropped and the pictures still follow the clock).
    #[derive(Default)]
    pub struct UltarPcmSink {
        card: Mutex<Option<Sink>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UltarPcmSink {
        const NAME: &'static str = "UltarPcmSink";
        type Type = super::UltarPcmSink;
        type ParentType = gstreamer_base::BaseSink;

        fn class_init(klass: &mut Self::Class) {
            klass.set_metadata(
                "Ultar PCM Sink",
                "Sink/Audio",
                "Writes raw 48 kHz 16-bit stereo audio to the Ultar sound card",
                "Ultar OS",
            );
        }
    }

    impl ObjectImpl for UltarPcmSink {}
    impl ElementImpl for UltarPcmSink {}

    impl BaseSinkImpl for UltarPcmSink {
        fn render(
            &self,
            buffer: &gstreamer::Buffer,
        ) -> Result<gstreamer::FlowSuccess, gstreamer::FlowError> {
            // the card is shared across the sink's streaming threads
            let mut card = self.card.lock().unwrap_or_else(|p| p.into_inner());
            if card.is_none() {
                *card = Some(Sink::open_default());
            }
            let Some(card) = card.as_mut() else {
                return Ok(gstreamer::FlowSuccess::Ok);
            };
            if !card.audible() {
                return Ok(gstreamer::FlowSuccess::Ok);
            }
            let map = buffer.map_readable().map_err(|_| gstreamer::FlowError::Error)?;
            let bytes: &[u8] = &map;
            let mut done = 0;
            while done < bytes.len() {
                let taken = card.offer(&bytes[done..]);
                if taken == 0 {
                    break; // the card is busy: drop the rest, stay ahead of the clock
                }
                done += taken;
            }
            Ok(gstreamer::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct UltarPcmSink(ObjectSubclass<imp::UltarPcmSink>)
        @extends gstreamer_base::BaseSink, gstreamer::Element, gstreamer::Object;
}

/// Registers the sink with the static plugin region so `autoaudiosink` (and with it Servo's
/// pipeline) considers it.
pub fn install() {
    let registered = gstreamer::Element::register(
        None,
        "ultarpcmsink",
        gstreamer::Rank::Primary,
        UltarPcmSink::static_type(),
    );
    if let Err(e) = registered {
        eprintln!("ultar-web-engine: audio sink not registered: {e}");
    }
}