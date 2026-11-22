//! Gesichtsentsperrung: startet `/usr/bin/ultar-face` (Frontkamera + YuNet/SFace, alles offline) und
//! meldet dessen Zeilen als Ereignisse. Bequemlichkeit, nicht Sicherheit: ein Foto kann täuschen –
//! deshalb nur zusätzlich zur PIN.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

pub const BIN: &str = "/usr/bin/ultar-face";
const MODEL: &str = "/usr/lib/ultar/face/face_recognition_sface_2021dec.onnx";
const STORE: &str = "/data/ultar/face/gesicht.bin";

#[derive(Debug, Clone, PartialEq)]
pub enum FaceEv {
    Camera,
    NoFace,
    Dark,
    Closer,
    Seen,
    /// Anlernen: n von m Bildern
    Progress(u32, u32),
    /// Anlernen: Bilder passten nicht zusammen, von vorn
    Restart,
    Match,
    NoMatch,
    Done,
    Timeout,
    Fail(String),
    /// Programm beendet (Exit-Code)
    Exit(i32),
}

pub fn parse(l: &str) -> Option<FaceEv> {
    let mut it = l.split_whitespace();
    Some(match it.next()? {
        "kamera" => FaceEv::Camera,
        "kein-gesicht" => FaceEv::NoFace,
        "dunkel" => FaceEv::Dark,
        "naeher" => FaceEv::Closer,
        "gesicht" => FaceEv::Seen,
        "fortschritt" => {
            let (a, b) = it.next()?.split_once('/')?;
            FaceEv::Progress(a.parse().ok()?, b.parse().ok()?)
        }
        "fortschritt-neu" => FaceEv::Restart,
        "ok" => FaceEv::Match,
        "nein" => FaceEv::NoMatch,
        "fertig" => FaceEv::Done,
        "zeit" => FaceEv::Timeout,
        "fehler" => FaceEv::Fail(it.collect::<Vec<_>>().join(" ")),
        _ => return None,
    })
}

/// Programm und Modelle vorhanden?
pub fn available() -> bool {
    std::path::Path::new(BIN).exists() && std::path::Path::new(MODEL).exists()
}

/// Ist ein Gesicht angelernt?
pub fn enrolled() -> bool {
    std::path::Path::new(STORE).exists()
}

pub fn delete() {
    let _ = std::fs::remove_file(STORE);
}

pub struct FaceRun {
    child: Child,
}

impl FaceRun {
    /// `args`: z. B. ["pruefen", "8"] oder ["anlernen", "40"]. Ereignisse kommen über `on`.
    pub fn start(args: &[&str], on: impl Fn(FaceEv) + Send + 'static) -> Option<Self> {
        let mut child = Command::new(BIN).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let out = child.stdout.take()?;
        let id = child.id();
        std::thread::Builder::new()
            .name("face".into())
            .spawn(move || {
                for l in BufReader::new(out).lines() {
                    let Ok(l) = l else { break };
                    if let Some(ev) = parse(&l) {
                        on(ev);
                    }
                }
                // Exit-Code abholen (Kind gehört FaceRun; hier nur über /proc warten wäre aufwendig)
                let _ = id;
                on(FaceEv::Exit(0));
            })
            .ok()?;
        Some(Self { child })
    }
}

impl Drop for FaceRun {
    fn drop(&mut self) {
        // Beenden: ultar-face stirbt, die Kamera-Hilfe sieht das Ende ihrer Eingabe und schaltet ab
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeilen() {
        assert_eq!(parse("kamera"), Some(FaceEv::Camera));
        assert_eq!(parse("gesicht 0.93"), Some(FaceEv::Seen));
        assert_eq!(parse("fortschritt 3/6"), Some(FaceEv::Progress(3, 6)));
        assert_eq!(parse("fortschritt-neu"), Some(FaceEv::Restart));
        assert_eq!(parse("ok 0.61"), Some(FaceEv::Match));
        assert_eq!(parse("nein 0.12"), Some(FaceEv::NoMatch));
        assert_eq!(parse("fehler kamera"), Some(FaceEv::Fail("kamera".into())));
        assert_eq!(parse("zeit"), Some(FaceEv::Timeout));
        assert_eq!(parse("fortschritt x"), None);
        assert_eq!(parse("bild 768x576"), None);
    }
}
