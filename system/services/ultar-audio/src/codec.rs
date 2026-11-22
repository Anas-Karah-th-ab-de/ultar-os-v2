//! Codec-Schnittstelle (Subpaket Audio-2 liefert die echte Cod3035-Implementierung).
//! Audio-1 nutzt nur dieses Trait + einen schlanken Stub, der die bekannten Mixer-Controls
//! des Cod3035 setzt (Speaker Mute aus, HP HP On, DAC Soft Mute aus, Speaker Volume).

use crate::mixer::Mixer;

pub trait Codec {
    /// Lautsprecher-Ausgang am Codec einschalten (DAC/AMP entsperren).
    fn enable_speaker(&self);
    /// Mikrofon-Eingang am Codec einschalten.
    fn enable_mic(&self);
    /// Lautsprecher-Lautstärke 0..1.
    fn set_speaker_volume(&self, v: f32);
}

/// Schlanker Stub über die rohen Cod3035-Mixer-Controls (bis Audio-2 den echten Codec liefert).
pub struct StubCodec;

impl Codec for StubCodec {
    fn enable_speaker(&self) {
        let m = Mixer::open();
        // dev-speaker (mixer_paths.xml): HP HP On=1 ; Speaker entsperren
        m.set_by_name("HP HP On", 1);
        m.set_by_name("Speaker Mute", 0); // "Speaker Mute Switch(1:muted_0:un)"
        m.set_by_name("DAC Soft Mute", 0);
        m.set_by_name("Force AMP Power Down", 0);
    }
    fn enable_mic(&self) {
        let m = Mixer::open();
        // Hauptmikrofon-Pfad (ADC Mixer MIC1 + MIC1 On)
        m.set_by_name("ADCL Mixer MIC1L Switch", 1);
        m.set_by_name("ADCR Mixer MIC1R Switch", 1);
        m.set_by_name("MIC1 MIC1 On", 1);
    }
    fn set_speaker_volume(&self, v: f32) {
        let m = Mixer::open();
        let val = (v.clamp(0.0, 1.0) * 167.0).round() as i64; // Speaker Volume [0..167]
        m.set_by_name("Speaker Volume", val);
    }
}
