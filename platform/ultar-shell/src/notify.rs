//! Benachrichtigungen in der Oberfläche: liest die von Apps/Diensten gemeldeten Ereignisse
//! (crate `ultar-notify`, Datei /run/ultar/notify.log) und alarmiert (Ton/Vibration), auch wenn
//! das Handy in der Tasche ist (Bildschirm aus). Zeigt die neueste als Kopf-Hinweis.

use ultar_notify::{Note, Urgency};

/// Verfolgt, bis wohin die Log-Datei schon gelesen wurde (Byte-Offset).
pub struct Notifier {
    seen: u64,
}

impl Default for Notifier {
    fn default() -> Self {
        // Beim Start am Ende beginnen, damit alte Zeilen nicht erneut alarmieren.
        let seen = std::fs::metadata(ultar_notify::LOG).map(|m| m.len()).unwrap_or(0);
        Self { seen }
    }
}

impl Notifier {
    /// Neue Benachrichtigungen seit dem letzten Aufruf.
    pub fn poll(&mut self) -> Vec<Note> {
        let len = std::fs::metadata(ultar_notify::LOG).map(|m| m.len()).unwrap_or(0);
        if len < self.seen {
            self.seen = 0; // Datei wurde geleert/rotiert
        }
        if len == self.seen {
            return Vec::new();
        }
        let data = match std::fs::read(ultar_notify::LOG) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };
        let slice = &data[self.seen.min(data.len() as u64) as usize..];
        let text = String::from_utf8_lossy(slice);
        let mut out = Vec::new();
        let mut consumed = 0usize;
        for line in text.split_inclusive('\n') {
            if !line.ends_with('\n') {
                break; // unvollständige letzte Zeile: nächstes Mal
            }
            consumed += line.len();
            if let Some(n) = ultar_notify::parse_line(line.trim_end()) {
                out.push(n);
            }
        }
        self.seen += consumed as u64;
        out
    }
}

/// Entscheidet, wie auf eine neue Benachrichtigung alarmiert wird.
pub struct Alert {
    pub sound: bool,
    pub vibrate_ms: u32,
    pub wake: bool,
}

pub fn alert_for(n: &Note, asleep: bool, sound_on: bool) -> Alert {
    match n.urgency {
        // Anruf o. Ä.: immer aufwecken, lauter Ton, kräftige Vibration
        Urgency::High => Alert { sound: sound_on, vibrate_ms: 600, wake: true },
        Urgency::Normal => Alert { sound: sound_on, vibrate_ms: if asleep { 250 } else { 0 }, wake: false },
        Urgency::Low => Alert { sound: false, vibrate_ms: 0, wake: false },
    }
}
