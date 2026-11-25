//! Stimme im Rust-Kern, ganz ohne Netz und ohne Sprachmodell: Äußerungen aus dem Mikrofonstrom schneiden,
//! Merkmale bilden (MFCC), mit eingesprochenen Mustern vergleichen (DTW) und die Stimme grob prüfen
//! (Klangfarbe + Tonhöhe).
//!
//! Das erkennt **eingelernte Wörter und Sätze des Besitzers** (Weckwort, geheimes Wort, eigene Befehle) –
//! es ist keine freie Spracherkennung. Die Stimmprüfung ist eine Bequemlichkeit, kein Schloss: eine gute
//! Aufnahme kann sie täuschen. Darum darf bei gesperrtem Gerät per Stimme nur Unkritisches geschehen.

use std::f32::consts::PI;

pub const NC: usize = 12;
pub type Frame = [f32; NC];
const RATE: f32 = 16_000.0;
const WIN: usize = 400; // 25 ms
const HOP: usize = 160; // 10 ms
const NFFT: usize = 512;
const NMEL: usize = 24;

/// Eine Äußerung: Merkmalsfolge (mittelwertbereinigt), Klangfarbe (Mittel der rohen Merkmale), Tonhöhe in Hz.
#[derive(Clone, Debug, PartialEq)]
pub struct Utt {
    pub mfcc: Vec<Frame>,
    pub mean: Frame,
    pub pitch: f32,
}

fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for s in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (s + k, s + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

fn mel(f: f32) -> f32 {
    2595.0 * (1.0 + f / 700.0).log10()
}

/// Rohe MFCC je 10 ms (ohne Mittelwertabzug).
fn mfcc_raw(s: &[i16]) -> Vec<Frame> {
    if s.len() < WIN {
        return Vec::new();
    }
    // Dreiecksfilter 100–7000 Hz
    let (lo, hi) = (mel(100.0), mel(7000.0));
    let edge: Vec<f32> = (0..NMEL + 2)
        .map(|i| {
            700.0 * (10f32.powf((lo + (hi - lo) * i as f32 / (NMEL + 1) as f32) / 2595.0) - 1.0)
                / RATE
                * NFFT as f32
        })
        .collect();
    let mut out = Vec::with_capacity((s.len() - WIN) / HOP + 1);
    let (mut re, mut im) = (vec![0f32; NFFT], vec![0f32; NFFT]);
    for start in (0..=s.len() - WIN).step_by(HOP) {
        re.fill(0.0);
        im.fill(0.0);
        for i in 0..WIN {
            let prev = if start + i > 0 {
                s[start + i - 1] as f32
            } else {
                0.0
            };
            let x = s[start + i] as f32 - 0.97 * prev;
            re[i] = x * (0.54 - 0.46 * (2.0 * PI * i as f32 / (WIN - 1) as f32).cos());
        }
        fft(&mut re, &mut im);
        let mut e = [0f32; NMEL];
        for (m, em) in e.iter_mut().enumerate() {
            let (a, b, c) = (edge[m], edge[m + 1], edge[m + 2]);
            let mut acc = 0.0;
            for k in a.floor() as usize..=(c.ceil() as usize).min(NFFT / 2) {
                let kf = k as f32;
                let w = if kf <= b {
                    (kf - a) / (b - a).max(1e-3)
                } else {
                    (c - kf) / (c - b).max(1e-3)
                };
                if w > 0.0 {
                    acc += w * (re[k] * re[k] + im[k] * im[k]);
                }
            }
            *em = (acc + 1.0).ln();
        }
        let mut f = [0f32; NC];
        for (c, fc) in f.iter_mut().enumerate() {
            *fc = (0..NMEL)
                .map(|m| e[m] * (PI * (c + 1) as f32 * (m as f32 + 0.5) / NMEL as f32).cos())
                .sum();
        }
        out.push(f);
    }
    out
}

/// Tonhöhe (Median über die stimmhaften Abschnitte), 0 = keine gefunden.
pub fn pitch(s: &[i16]) -> f32 {
    let mut found = Vec::new();
    let (lo, hi) = ((RATE / 400.0) as usize, (RATE / 70.0) as usize);
    for w in s.chunks_exact(640) {
        let e0: f32 = w.iter().map(|v| (*v as f32).powi(2)).sum();
        if e0 / 640.0 < 40.0 * 40.0 {
            continue;
        }
        let (mut best, mut lag) = (0.0f32, 0usize);
        for l in lo..=hi.min(639) {
            let c: f32 = (0..640 - l).map(|i| w[i] as f32 * w[i + l] as f32).sum();
            let n = c / e0;
            if n > best {
                best = n;
                lag = l;
            }
        }
        if best > 0.45 && lag > 0 {
            found.push(RATE / lag as f32);
        }
    }
    if found.is_empty() {
        return 0.0;
    }
    found.sort_by(|a, b| a.total_cmp(b));
    found[found.len() / 2]
}

/// Äußerung auswerten; None, wenn zu kurz (< 0,25 s).
pub fn analyze(s: &[i16]) -> Option<Utt> {
    let raw = mfcc_raw(s);
    if raw.len() < 25 {
        return None;
    }
    let mut mean = [0f32; NC];
    for f in &raw {
        for c in 0..NC {
            mean[c] += f[c] / raw.len() as f32;
        }
    }
    let mfcc = raw
        .iter()
        .map(|f| std::array::from_fn(|c| f[c] - mean[c]))
        .collect();
    Some(Utt {
        mfcc,
        mean,
        pitch: pitch(s),
    })
}

fn dist(a: &Frame, b: &Frame) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

/// Abstand zweier Merkmalsfolgen (dynamische Zeitanpassung mit Band), je Schritt gemittelt.
pub fn dtw(a: &[Frame], b: &[Frame]) -> f32 {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 || n > 2 * m + 10 || m > 2 * n + 10 {
        return f32::MAX;
    }
    let band = (n.max(m) / 4).max(n.abs_diff(m) + 5);
    let mut prev = vec![f32::MAX; m + 1];
    let mut cur = vec![f32::MAX; m + 1];
    prev[0] = 0.0;
    for i in 1..=n {
        cur.fill(f32::MAX);
        let c = i * m / n;
        for j in c.saturating_sub(band).max(1)..=(c + band).min(m) {
            let best = prev[j].min(prev[j - 1]).min(cur[j - 1]);
            if best < f32::MAX {
                cur[j] = best + dist(&a[i - 1], &b[j - 1]);
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m] / (n + m) as f32
}

/// Schneidet Äußerungen aus dem laufenden Strom (Blöcke von 20 ms, 16 kHz).
pub struct Cutter {
    noise: f32,
    buf: Vec<i16>,
    pre: Vec<i16>,
    speech: usize,
    quiet: usize,
    /// Pegel des letzten Blocks (für die Anzeige)
    pub level: f32,
}

impl Default for Cutter {
    fn default() -> Self {
        Cutter {
            noise: 30.0,
            buf: Vec::new(),
            pre: Vec::new(),
            speech: 0,
            quiet: 0,
            level: 0.0,
        }
    }
}

impl Cutter {
    /// Block einspeisen; liefert eine fertige Äußerung, sobald nach Sprache 0,4 s Ruhe war (höchstens 4 s lang).
    pub fn feed(&mut self, block: &[i16]) -> Option<Vec<i16>> {
        let rms = (block.iter().map(|v| (*v as f32).powi(2)).sum::<f32>()
            / block.len().max(1) as f32)
            .sqrt();
        self.level = rms;
        let loud = rms > (self.noise * 3.0).max(45.0);
        if self.buf.is_empty() {
            if loud {
                self.buf.extend_from_slice(&self.pre);
                self.buf.extend_from_slice(block);
                self.speech = 1;
                self.quiet = 0;
            } else {
                // Grundrauschen nachführen, 0,2 s Vorlauf behalten
                self.noise = self.noise * 0.95 + rms * 0.05;
                self.pre.extend_from_slice(block);
                let keep = 3200;
                if self.pre.len() > keep {
                    self.pre.drain(..self.pre.len() - keep);
                }
            }
            return None;
        }
        self.buf.extend_from_slice(block);
        if loud {
            self.speech += 1;
            self.quiet = 0;
        } else {
            self.quiet += 1;
        }
        if self.quiet >= 20 || self.buf.len() >= 4 * 16_000 {
            let mut out = std::mem::take(&mut self.buf);
            self.pre.clear();
            let ok = self.speech >= 12; // mindestens ~0,25 s Sprache
            self.speech = 0;
            // Ruhe am Ende bis auf 0,1 s abschneiden
            out.truncate(
                out.len()
                    .saturating_sub(self.quiet.saturating_sub(5) * block.len()),
            );
            self.quiet = 0;
            return ok.then_some(out);
        }
        None
    }
}

/// Eingelernter Satz: Name, Anweisung (Text, den der Helfer ausführt) und die Muster.
#[derive(Clone, Debug, Default)]
pub struct Phrase {
    pub name: String,
    pub action: String,
    pub refs: Vec<Utt>,
    pub thr: f32,
}

/// Feste Namen der besonderen Sätze
pub const WAKE: &str = "wake";
pub const SECRET: &str = "secret";

#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub phrases: Vec<Phrase>,
    /// Klangfarbe und Tonhöhe des Besitzers (aus allen eingesprochenen Mustern)
    pub mean: Frame,
    pub pitch: f32,
    pub spread: f32,
}

#[derive(Debug, PartialEq)]
pub enum Heard {
    /// Satz erkannt, Stimme passt
    Owner(usize),
    /// Satz erkannt, aber die Stimme passt nicht zum Besitzer
    Stranger(usize),
    Nothing,
}

impl Profile {
    /// Satz neu einlernen (ersetzt einen gleichnamigen). Schwelle aus der Streuung der Muster untereinander.
    pub fn enroll(&mut self, name: &str, action: &str, refs: Vec<Utt>) {
        let mut ds = Vec::new();
        for i in 0..refs.len() {
            for j in i + 1..refs.len() {
                ds.push(dtw(&refs[i].mfcc, &refs[j].mfcc));
            }
        }
        let intra = if ds.is_empty() {
            6.0
        } else {
            ds.iter().sum::<f32>() / ds.len() as f32
        };
        let p = Phrase {
            name: name.into(),
            action: action.into(),
            refs,
            thr: (intra * 2.0).max(intra + 4.0),
        };
        match self.phrases.iter().position(|x| x.name == name) {
            Some(i) => self.phrases[i] = p,
            None => self.phrases.push(p),
        }
        self.refit();
    }

    #[allow(dead_code)] // not used by anything yet; kept, see docs/OFFEN-WARNUNGEN.md
    pub fn forget(&mut self, name: &str) {
        self.phrases.retain(|p| p.name != name);
        self.refit();
    }

    fn refit(&mut self) {
        let all: Vec<&Utt> = self.phrases.iter().flat_map(|p| p.refs.iter()).collect();
        if all.is_empty() {
            *self = Profile::default();
            return;
        }
        self.mean =
            std::array::from_fn(|c| all.iter().map(|u| u.mean[c]).sum::<f32>() / all.len() as f32);
        let mut ps: Vec<f32> = all.iter().map(|u| u.pitch).filter(|p| *p > 0.0).collect();
        ps.sort_by(|a, b| a.total_cmp(b));
        self.pitch = ps.get(ps.len() / 2).copied().unwrap_or(0.0);
        let far = all
            .iter()
            .map(|u| dist(&u.mean, &self.mean))
            .fold(0.0f32, f32::max);
        self.spread = (far * 1.8).max(9.0);
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.phrases.iter().position(|p| p.name == name)
    }

    /// Passt die Stimme zum Besitzer?
    pub fn same_voice(&self, u: &Utt) -> bool {
        let timbre = dist(&u.mean, &self.mean) <= self.spread;
        let tone = self.pitch <= 0.0
            || u.pitch <= 0.0
            || (u.pitch / self.pitch > 0.78 && u.pitch / self.pitch < 1.28);
        timbre && tone
    }

    /// Messwerte einer Äußerung gegen alle Sätze (für das Protokoll zur Feinabstimmung).
    pub fn scores(&self, u: &Utt) -> String {
        let per: Vec<String> = self
            .phrases
            .iter()
            .map(|p| {
                format!(
                    "{}={:.2}",
                    p.name,
                    p.refs
                        .iter()
                        .map(|r| dtw(&r.mfcc, &u.mfcc))
                        .fold(f32::MAX, f32::min)
                        / p.thr.max(0.1)
                )
            })
            .collect();
        format!(
            "stimme {:.1}/{:.1} ton {:.0}/{:.0} {}",
            dist(&u.mean, &self.mean),
            self.spread,
            u.pitch,
            self.pitch,
            per.join(" ")
        )
    }

    /// Äußerung einordnen. `only`: nur diese Sätze prüfen (z. B. nur das Weckwort).
    pub fn hear(&self, u: &Utt, only: impl Fn(&Phrase) -> bool) -> Heard {
        let mut best: Option<(usize, f32)> = None;
        for (i, p) in self.phrases.iter().enumerate().filter(|(_, p)| only(p)) {
            let d = p
                .refs
                .iter()
                .map(|r| dtw(&r.mfcc, &u.mfcc))
                .fold(f32::MAX, f32::min)
                / p.thr.max(0.1);
            if best.is_none_or(|b| d < b.1) {
                best = Some((i, d));
            }
        }
        match best {
            Some((i, d)) if d <= 1.0 && self.same_voice(u) => Heard::Owner(i),
            // Wort passt (auch etwas lockerer – fremde Stimmen klingen anders), Stimme nicht
            Some((i, d)) if d <= 1.6 && !self.same_voice(u) => Heard::Stranger(i),
            _ => Heard::Nothing,
        }
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for p in &self.phrases {
            s.push_str(&format!(
                "P\t{}\t{}\t{}\n",
                p.name,
                p.action.replace(['\t', '\n'], " "),
                p.thr
            ));
            for r in &p.refs {
                let nums: Vec<String> = r
                    .mean
                    .iter()
                    .chain(r.mfcc.iter().flatten())
                    .map(|v| format!("{v:.2}"))
                    .collect();
                s.push_str(&format!("R\t{}\t{}\n", r.pitch, nums.join(" ")));
            }
        }
        s
    }

    pub fn from_text(t: &str) -> Profile {
        let mut pr = Profile::default();
        for l in t.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            match f.as_slice() {
                ["P", name, action, thr] => pr.phrases.push(Phrase {
                    name: name.to_string(),
                    action: action.to_string(),
                    refs: Vec::new(),
                    thr: thr.parse().unwrap_or(8.0),
                }),
                ["R", pitch, nums] => {
                    let v: Vec<f32> = nums.split(' ').filter_map(|x| x.parse().ok()).collect();
                    if v.len() >= NC * 2 && v.len() % NC == 0 {
                        let fr = |i: usize| -> Frame { std::array::from_fn(|c| v[i * NC + c]) };
                        if let Some(p) = pr.phrases.last_mut() {
                            p.refs.push(Utt {
                                mean: fr(0),
                                mfcc: (1..v.len() / NC).map(fr).collect(),
                                pitch: pitch.parse().unwrap_or(0.0),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        pr.refit();
        pr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// künstliche „Stimme": Grundton mit Obertönen, deren Gewichtung (Klangfarbe) über die Silben wechselt
    fn say(f0: f32, sylls: &[(f32, f32)], jitter: f32) -> Vec<i16> {
        let mut out = Vec::new();
        let mut ph = 0.0f32;
        for (k, (bright, len)) in sylls.iter().enumerate() {
            let n = (RATE * len * (1.0 + jitter * if k % 2 == 0 { 1.0 } else { -1.0 })) as usize;
            for i in 0..n {
                ph += 2.0 * PI * f0 / RATE;
                let env = (PI * i as f32 / n as f32).sin();
                let mut v = 0.0;
                for h in 1..=40 {
                    let hf = h as f32;
                    // Formant bei `bright` Hz – unabhängig vom Grundton, wie bei einer echten Stimme
                    v += (ph * hf).sin() * (-((hf * f0 - bright) / 350.0).powi(2)).exp();
                }
                out.push((v * env * 6000.0) as i16);
            }
            out.extend(std::iter::repeat_n(0i16, 800));
        }
        out
    }

    #[test]
    fn woerter_und_stimmen() {
        let hey = [(500.0, 0.18), (1800.0, 0.22), (900.0, 0.2)];
        let other = [(2200.0, 0.3), (600.0, 0.15), (2600.0, 0.12), (1200.0, 0.2)];
        let refs: Vec<Utt> = [0.0, 0.06, -0.05]
            .iter()
            .map(|j| analyze(&say(120.0, &hey, *j)).unwrap())
            .collect();
        let mut p = Profile::default();
        p.enroll(WAKE, "", refs);
        let all = |_: &Phrase| true;
        // dasselbe Wort, etwas anders gesprochen: erkannt
        assert_eq!(
            p.hear(&analyze(&say(121.0, &hey, 0.03)).unwrap(), all),
            Heard::Owner(0)
        );
        // anderes Wort: nichts
        assert_eq!(
            p.hear(&analyze(&say(120.0, &other, 0.0)).unwrap(), all),
            Heard::Nothing
        );
        // dasselbe Wort, deutlich andere Tonhöhe: fremde Stimme
        assert_eq!(
            p.hear(&analyze(&say(160.0, &hey, 0.0)).unwrap(), all),
            Heard::Stranger(0)
        );
        // speichern und laden
        let q = Profile::from_text(&p.to_text());
        assert_eq!(q.phrases.len(), 1);
        assert_eq!(
            q.hear(&analyze(&say(119.0, &hey, -0.02)).unwrap(), all),
            Heard::Owner(0)
        );
        let pt = pitch(&say(150.0, &hey, 0.0));
        assert!((pt - 150.0).abs() < 6.0, "{pt}");
    }

    #[test]
    fn schneiden() {
        let mut c = Cutter::default();
        let mut got = Vec::new();
        let mut stream = vec![0i16; 16_000];
        stream.extend(say(130.0, &[(700.0, 0.25), (1500.0, 0.25)], 0.0));
        stream.extend(vec![0i16; 16_000]);
        for b in stream.chunks_exact(320) {
            if let Some(u) = c.feed(b) {
                got.push(u);
            }
        }
        assert_eq!(got.len(), 1);
        assert!(
            got[0].len() > 6000 && got[0].len() < 16_000,
            "{}",
            got[0].len()
        );
        assert!(analyze(&got[0]).is_some());
    }
}
