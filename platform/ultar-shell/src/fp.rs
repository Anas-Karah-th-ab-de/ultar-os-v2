//! Fingerabdruck über Samsungs Android-Dienst: Der Dauerdienst `fpclient daemon` (tools/android-fp)
//! hält die Verbindung zum Fingerabdruck-HAL und spricht ein einfaches Zeilenprotokoll über
//! `/run/ultar/fp.sock`. Ultar bekommt nur Ergebnisse („erkannt: ja/nein"), nie Fingerdaten.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub const SOCK: &str = "/run/ultar/fp.sock";

#[derive(Debug, Clone, PartialEq)]
pub enum FpEv {
    /// Finger erkannt (Finger-Nummer)
    Match(u32),
    /// Finger aufgelegt, aber nicht erkannt
    NoMatch,
    /// Sensor meldet etwas (0 = gut, 1–5 = Teilbild/unsauber/zu schnell …)
    Acquired(i32),
    /// Anlernen: noch n Berührungen
    Rest(u32),
    /// Anlernen fertig (Finger-Nummer)
    Learned(u32),
    /// Anlernen hat begonnen
    Started,
    /// Anzahl angelernter Finger
    Count(u32),
    Removed,
    /// Fehler (Android-Fehlercode, Herstellercode) – 3 = Zeit abgelaufen, 7 = zu viele Versuche
    Error(i32, i32),
    Fail(String),
    Closed,
}

/// Eine Antwortzeile des Dienstes deuten.
pub fn parse(l: &str) -> Option<FpEv> {
    let mut it = l.split_whitespace();
    let k = it.next()?;
    let n = |s: Option<&str>| s.and_then(|v| v.parse::<i64>().ok());
    Some(match k {
        "ok" => FpEv::Match(n(it.next())? as u32),
        "nein" => FpEv::NoMatch,
        "acq" => FpEv::Acquired(n(it.next())? as i32),
        "rest" => FpEv::Rest(n(it.next())? as u32),
        "fertig" => FpEv::Learned(n(it.next())? as u32),
        "los" => FpEv::Started,
        "anzahl" => FpEv::Count(n(it.next())? as u32),
        "entfernt" => FpEv::Removed,
        "fehler" => FpEv::Error(n(it.next())? as i32, n(it.next()).unwrap_or(0) as i32),
        "err" => FpEv::Fail(it.collect::<Vec<_>>().join(" ")),
        _ => return None,
    })
}

/// Ist der Fingerabdruck-Dienst auf diesem Gerät vorhanden?
pub fn available() -> bool {
    std::path::Path::new(SOCK).exists()
}

pub struct FpConn {
    w: UnixStream,
}

impl FpConn {
    /// Verbinden; Ereignisse kommen über `on` (aus einem eigenen Thread).
    pub fn open(on: impl Fn(FpEv) + Send + 'static) -> Option<Self> {
        let s = UnixStream::connect(SOCK).ok()?;
        let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
        let r = s.try_clone().ok()?;
        std::thread::Builder::new()
            .name("fp".into())
            .spawn(move || {
                for l in BufReader::new(r).lines() {
                    let Ok(l) = l else { break };
                    if let Some(ev) = parse(&l) {
                        on(ev);
                    }
                }
                on(FpEv::Closed);
            })
            .ok()?;
        Some(Self { w: s })
    }

    pub fn send(&mut self, cmd: &str) -> bool {
        self.w.write_all(format!("{cmd}\n").as_bytes()).is_ok()
    }
}

impl Drop for FpConn {
    fn drop(&mut self) {
        // Schließen bricht laufendes Prüfen/Anlernen im Dienst ab
        let _ = self.w.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeilen() {
        assert_eq!(parse("ok 3"), Some(FpEv::Match(3)));
        assert_eq!(parse("nein"), Some(FpEv::NoMatch));
        assert_eq!(parse("acq 1"), Some(FpEv::Acquired(1)));
        assert_eq!(parse("rest 11"), Some(FpEv::Rest(11)));
        assert_eq!(parse("fertig 2"), Some(FpEv::Learned(2)));
        assert_eq!(parse("anzahl 0"), Some(FpEv::Count(0)));
        assert_eq!(parse("fehler 3 0"), Some(FpEv::Error(3, 0)));
        assert_eq!(parse("fehler 8"), Some(FpEv::Error(8, 0)));
        assert_eq!(
            parse("err gatekeeper verify"),
            Some(FpEv::Fail("gatekeeper verify".into()))
        );
        assert_eq!(parse("los"), Some(FpEv::Started));
        assert_eq!(parse("quatsch"), None);
        assert_eq!(parse("ok"), None);
    }
}
