//! Sound (and later video) of pages through GStreamer, behind the `media` feature. The sound
//! sink writes raw audio to the sound card of the Ultar kernel (`/dev/ultar/pcm`, 48 kHz · 16
//! bit · stereo) with non-blocking writes; the card logic is [`crate::pcm`], shared with the
//! browser's media service and unit-tested on its own.
//!
//! Without the feature nothing is registered and the engine stays exactly as it is for pages
//! without sound; on Linux (no `/dev/ultar/pcm`) the card is a no-op and the pictures keep
//! following the clock.

/// Starts GStreamer and registers the Ultar audio sink (ranked above the distributions' sinks)
/// before Servo starts (only with `media`; otherwise nothing happens).
pub fn install() {
    #[cfg(feature = "media")]
    gst::install();
}

#[cfg(feature = "media")]
mod gst;
