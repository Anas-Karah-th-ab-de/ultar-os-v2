//! Samsungs Tonwege aus /vendor/etc/mixer_paths.xml (+ mixer_gains.xml) lesen und anwenden –
//! dieselbe Logik wie Androids audio_route: Grundeinstellung = Regler auf oberster Ebene,
//! ein Weg = Liste von Reglern und Verweisen auf andere Wege (rekursiv).

use crate::ctl::Ctl;
use std::collections::HashMap;

pub const PATHS: &str = "/vendor/etc/mixer_paths.xml";

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Ctl(String, String),
    Path(String),
}

#[derive(Default)]
pub struct Paths {
    pub defaults: Vec<(String, String)>,
    pub paths: HashMap<String, Vec<Step>>,
}

fn attr(tag: &str, key: &str) -> Option<String> {
    let k = format!("{key}=\"");
    let s = tag.find(&k)? + k.len();
    let e = tag[s..].find('"')? + s;
    Some(tag[s..e].to_string())
}

impl Paths {
    pub fn load() -> Paths {
        let mut p = Paths::default();
        p.parse_file(PATHS, 0);
        p
    }

    fn parse_file(&mut self, file: &str, depth: u32) {
        if depth > 3 {
            return;
        }
        if let Ok(s) = std::fs::read_to_string(file) {
            self.parse(&s, depth);
        }
    }

    pub fn parse(&mut self, xml: &str, depth: u32) {
        // Kommentare entfernen
        let mut s = String::with_capacity(xml.len());
        let mut rest = xml;
        while let Some(a) = rest.find("<!--") {
            s.push_str(&rest[..a]);
            rest = match rest[a..].find("-->") {
                Some(e) => &rest[a + e + 3..],
                None => "",
            };
        }
        s.push_str(rest);
        // Stapel offener <path>-Blöcke
        let mut stack: Vec<String> = Vec::new();
        let mut i = 0;
        while let Some(a) = s[i..].find('<') {
            let a = i + a;
            let Some(e) = s[a..].find('>') else { break };
            let tag = &s[a + 1..a + e];
            i = a + e + 1;
            let selfclose = tag.ends_with('/');
            let t = tag.trim_end_matches('/').trim();
            if t.starts_with("/path") {
                stack.pop();
            } else if t.starts_with("path ") {
                let name = attr(t, "name").unwrap_or_default();
                if selfclose {
                    if let Some(cur) = stack.last() {
                        self.paths
                            .entry(cur.clone())
                            .or_default()
                            .push(Step::Path(name));
                    }
                } else {
                    // Ein Weg darf mehrfach vorkommen (gains ergänzt nicht, sondern ist eigener Name) – neu anlegen
                    self.paths.insert(name.clone(), Vec::new());
                    stack.push(name);
                }
            } else if t.starts_with("ctl ") {
                let (Some(n), Some(v)) = (attr(t, "name"), attr(t, "value")) else {
                    continue;
                };
                match stack.last() {
                    Some(cur) => self
                        .paths
                        .entry(cur.clone())
                        .or_default()
                        .push(Step::Ctl(n, v)),
                    None => self.defaults.push((n, v)),
                }
            } else if t.starts_with("include ") {
                if let Some(f) = attr(t, "name") {
                    self.parse_file(&f, depth + 1);
                }
            }
        }
    }

    /// Weg in eine flache Reglerliste auflösen.
    pub fn flatten(&self, name: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.walk(name, &mut out, 0);
        out
    }

    fn walk(&self, name: &str, out: &mut Vec<(String, String)>, depth: u32) {
        if depth > 16 {
            return;
        }
        let Some(steps) = self.paths.get(name) else {
            return;
        };
        for s in steps {
            match s {
                Step::Ctl(n, v) => out.push((n.clone(), v.clone())),
                Step::Path(p) => self.walk(p, out, depth + 1),
            }
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.paths.contains_key(name)
    }

    /// Grundeinstellung setzen. Liefert Fehlerzahl.
    pub fn reset(&self, c: &Ctl, verbose: bool) -> usize {
        apply(c, &self.defaults, verbose)
    }

    /// Nur die Wiedergabe-Grundeinstellung (Aufnahmeregler bleiben, damit eine laufende Aufnahme weiterläuft).
    pub fn reset_playback(&self, c: &Ctl, verbose: bool) -> usize {
        let capture = |n: &str| {
            ["NSRC", "SPUM", "SIFM", "RECP", "MIC", "ADC", "Inmux"]
                .iter()
                .any(|k| n.contains(k))
        };
        let list: Vec<(String, String)> = self
            .defaults
            .iter()
            .filter(|(n, _)| !capture(n))
            .cloned()
            .collect();
        apply(c, &list, verbose)
    }

    /// Weg (und, falls vorhanden, dessen Lautstärke „gain-<name>") anwenden.
    pub fn apply_path(&self, c: &Ctl, name: &str, verbose: bool) -> usize {
        let mut list = self.flatten(name);
        list.extend(self.flatten(&format!("gain-{name}")));
        if list.is_empty() && !self.has(name) {
            if verbose {
                println!("Weg unbekannt: {name}");
            }
            return 1;
        }
        apply(c, &list, verbose)
    }
}

pub fn apply(c: &Ctl, list: &[(String, String)], verbose: bool) -> usize {
    let mut err = 0;
    for (n, v) in list {
        match c.set(n, v) {
            Ok(()) => {
                if verbose {
                    println!("  {n} = {v}");
                }
            }
            Err(e) => {
                err += 1;
                if verbose {
                    println!("  ! {e}");
                }
            }
        }
    }
    err
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_nested() {
        let x = r#"<mixer>
<!-- c <ctl name="X" value="9" /> -->
<ctl name="A" value="0" />
<path name="dev-h"><ctl name="EP EP On" value="1" /></path>
<path name="inc-h">
  <path name="dev-h" />
  <ctl name="ABOX Sound Type" value="VOICE" />
</path>
<path name="gain-inc-h"><ctl name="DAC Gain" value="150" /></path>
</mixer>"#;
        let mut p = Paths::default();
        p.parse(x, 0);
        assert_eq!(p.defaults, vec![("A".to_string(), "0".to_string())]);
        assert_eq!(
            p.flatten("inc-h"),
            vec![
                ("EP EP On".into(), "1".into()),
                ("ABOX Sound Type".into(), "VOICE".into())
            ]
        );
        assert_eq!(p.flatten("gain-inc-h").len(), 1);
    }
}
