//! Rechenkern-Schnittstelle. Eine `Session` = ein geladenes Modell mit Kontext; sie lebt nur im
//! Arbeits-Thread des Dienstes. Implementierungen: llama.cpp (`llama.rs`) und eine Test-Engine (`mock:`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Msg {
    pub role: String,
    pub content: String,
    /// Bilder zu dieser Nachricht (Dateipfade) – nur Modelle mit Projektor (mmproj) sehen sie
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl Msg {
    pub fn text(role: &str, content: &str) -> Self {
        Msg { role: role.into(), content: content.into(), images: Vec::new() }
    }
    /// Inhalt mit einer Bildmarke je Bild davor (für Modelle mit Projektor)
    pub fn content_mm(&self) -> String {
        if self.images.is_empty() {
            return self.content.clone();
        }
        let marks: String = self.images.iter().map(|_| format!("{MEDIA_MARKER}\n")).collect();
        format!("{marks}{}", self.content)
    }
}

#[derive(Clone, Debug)]
pub struct GenParams {
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub min_p: f32,
    pub repeat_penalty: f32,
    pub seed: u32,
    pub stop: Vec<String>,
    /// GBNF-Grammatik (z. B. für reines JSON)
    pub grammar: Option<String>,
}

impl Default for GenParams {
    fn default() -> Self {
        Self { max_tokens: 512, temperature: 0.7, top_p: 0.9, top_k: 40, min_p: 0.05, repeat_penalty: 1.05, seed: 0xC0DE, stop: vec![], grammar: None }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Stats {
    pub prompt_tokens: u32,
    /// davon aus dem Zwischenspeicher übernommen (nicht neu gerechnet)
    pub cached_tokens: u32,
    pub completion_tokens: u32,
    pub prompt_ms: f64,
    pub gen_ms: f64,
    /// "stop" (Ende/Stoppwort) oder "length" (max_tokens/Kontext voll)
    pub finish: String,
}

impl Stats {
    pub fn prompt_tps(&self) -> f64 {
        let n = self.prompt_tokens.saturating_sub(self.cached_tokens) as f64;
        if self.prompt_ms > 0.0 {
            n * 1000.0 / self.prompt_ms
        } else {
            0.0
        }
    }
    pub fn gen_tps(&self) -> f64 {
        if self.gen_ms > 0.0 {
            self.completion_tokens as f64 * 1000.0 / self.gen_ms
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub path: String,
    pub engine: String,
    pub n_ctx: u32,
    pub n_params: u64,
    pub bytes: u64,
    pub threads: i32,
    pub load_ms: f64,
}

pub trait Session {
    fn info(&self) -> ModelInfo;
    /// Chatverlauf → Prompt-Text nach der Vorlage des Modells (endet mit dem Anfang der Assistenten-Antwort).
    fn chat_prompt(&self, msgs: &[Msg], json_only: bool) -> Result<String, String>;
    /// Text erzeugen. `sink` bekommt fertige Textstücke; gibt sie `false` zurück, wird abgebrochen.
    fn generate(&mut self, prompt: &str, p: &GenParams, sink: &mut dyn FnMut(&str) -> bool) -> Result<Stats, String>;
    /// Kann das Modell Bilder sehen (Projektor geladen)?
    fn vision(&self) -> bool {
        false
    }
    /// Erzeugen mit Bildern: `prompt` enthält für jedes Bild die Marke `MEDIA_MARKER` (in Reihenfolge von `images`).
    fn generate_mm(&mut self, _prompt: &str, _images: &[String], _p: &GenParams, _sink: &mut dyn FnMut(&str) -> bool) -> Result<Stats, String> {
        Err("this model cannot see images – install a vision model (e.g. ultar-llm pull qwen2.5-vl-3b)".into())
    }
}

/// Platzhalter für ein Bild im Prompt (wie llama.cpp mtmd)
pub const MEDIA_MARKER: &str = "<__media__>";

/// Grammatik für genau ein JSON-Objekt (aus llama.cpp grammars/json.gbnf).
pub const JSON_GBNF: &str = r#"root   ::= object
value  ::= object | array | string | number | ("true" | "false" | "null") ws
object ::= "{" ws ( string ":" ws value ("," ws string ":" ws value)* )? "}" ws
array  ::= "[" ws ( value ("," ws value)* )? "]" ws
string ::= "\"" ( [^"\\\x7F\x00-\x1F] | "\\" (["\\bfnrt] | "u" [0-9a-fA-F]{4}) )* "\"" ws
number ::= ("-"? ([0-9] | [1-9] [0-9]{0,15})) ("." [0-9]+)? ([eE] [-+]? [0-9] [1-9]{0,15})? ws
ws ::= | " " | "\n" [ \t]{0,20}
"#;

/// Zerlegt den erzeugten Text so, dass Stoppwörter nie (auch nicht teilweise) beim Empfänger ankommen
/// und UTF-8 nie mitten im Zeichen getrennt wird.
pub struct StopFilter {
    stops: Vec<String>,
    hold: usize,
    out: String,
    sent: usize,
    pending: Vec<u8>,
}

impl StopFilter {
    pub fn new(stops: &[String]) -> Self {
        let stops: Vec<String> = stops.iter().filter(|s| !s.is_empty()).cloned().collect();
        let hold = stops.iter().map(|s| s.len()).max().unwrap_or(1).saturating_sub(1);
        Self { stops, hold, out: String::new(), sent: 0, pending: Vec::new() }
    }

    /// Neue Bytes eines Tokens. Liefert (freizugebender Text, Stoppwort gefunden).
    pub fn push(&mut self, bytes: &[u8]) -> (String, bool) {
        self.pending.extend_from_slice(bytes);
        // gültiges UTF-8 übernehmen, angefangene Zeichen zurückhalten
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(s) => s.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(e) => {
                // wirklich ungültig: ersetzen und weiter
                let n = e.valid_up_to() + e.error_len().unwrap_or(1);
                let s = String::from_utf8_lossy(&self.pending[..n]).into_owned();
                self.out.push_str(&s);
                self.pending.drain(..n);
                return self.release();
            }
        };
        let s = std::str::from_utf8(&self.pending[..valid]).unwrap().to_string();
        self.pending.drain(..valid);
        self.out.push_str(&s);
        self.release()
    }

    fn release(&mut self) -> (String, bool) {
        let search_from = self.sent.saturating_sub(self.hold);
        for st in &self.stops {
            if let Some(i) = self.out[search_from..].find(st.as_str()) {
                let end = search_from + i;
                let r = self.out[self.sent..end.max(self.sent)].to_string();
                self.out.truncate(end);
                self.sent = self.out.len();
                return (r, true);
            }
        }
        let mut safe = self.out.len().saturating_sub(self.hold);
        while safe > self.sent && !self.out.is_char_boundary(safe) {
            safe -= 1;
        }
        if safe <= self.sent {
            return (String::new(), false);
        }
        let r = self.out[self.sent..safe].to_string();
        self.sent = safe;
        (r, false)
    }

    /// Rest am Ende freigeben.
    pub fn finish(&mut self) -> String {
        if !self.pending.is_empty() {
            let s = String::from_utf8_lossy(&self.pending).into_owned();
            self.pending.clear();
            self.out.push_str(&s);
        }
        let r = self.out[self.sent..].to_string();
        self.sent = self.out.len();
        r
    }

    pub fn text(&self) -> &str {
        &self.out
    }
}

/// Einfache ChatML-Vorlage (Qwen, viele andere) als Rückfall, wenn das Modell keine eigene mitbringt.
pub fn chatml(msgs: &[Msg]) -> String {
    let mut s = String::new();
    for m in msgs {
        s.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", m.role, m.content_mm()));
    }
    s.push_str("<|im_start|>assistant\n");
    s
}

/// Test-Engine: braucht kein Modell, antwortet deterministisch. Dient Tests, CI und dem Prüfen der Schnittstelle.
pub struct Mock {
    pub n_ctx: u32,
}

impl Session for Mock {
    fn info(&self) -> ModelInfo {
        ModelInfo { id: "mock".into(), path: "mock:".into(), engine: "mock".into(), n_ctx: self.n_ctx, n_params: 0, bytes: 0, threads: 1, load_ms: 0.0 }
    }

    fn chat_prompt(&self, msgs: &[Msg], _json_only: bool) -> Result<String, String> {
        Ok(chatml(msgs))
    }

    fn vision(&self) -> bool {
        true
    }

    /// Test: meldet, welche Bilder angekommen sind (Größe aus dem Dateikopf), ohne zu „sehen"
    fn generate_mm(&mut self, prompt: &str, images: &[String], p: &GenParams, sink: &mut dyn FnMut(&str) -> bool) -> Result<Stats, String> {
        if prompt.matches(MEDIA_MARKER).count() != images.len() {
            return Err("image markers do not match images".into());
        }
        let mut seen = Vec::new();
        for i in images {
            let b = std::fs::read(i).map_err(|e| format!("{i}: {e}"))?;
            seen.push(format!("{} bytes", b.len()));
        }
        let text = format!("mock saw {} image(s): {}", images.len(), seen.join(", "));
        let text = if p.grammar.is_some() { serde_json::json!({"answer": text}).to_string() } else { text };
        sink(&text);
        Ok(Stats { prompt_tokens: 1, completion_tokens: 1, finish: "stop".into(), ..Default::default() })
    }

    fn generate(&mut self, prompt: &str, p: &GenParams, sink: &mut dyn FnMut(&str) -> bool) -> Result<Stats, String> {
        let t0 = std::time::Instant::now();
        // letzte Nutzerzeile finden
        let user = prompt.rsplit("<|im_start|>user\n").next().and_then(|r| r.split("<|im_end|>").next()).unwrap_or(prompt).trim().to_string();
        let text = if p.grammar.is_some() { serde_json::json!({"answer": format!("echo: {user}")}).to_string() } else { format!("echo: {user}") };
        let mut f = StopFilter::new(&p.stop);
        let mut st = Stats { prompt_tokens: prompt.split_whitespace().count() as u32, ..Default::default() };
        st.finish = "length".into();
        for (i, ch) in text.split_inclusive(' ').enumerate() {
            if i as u32 >= p.max_tokens {
                break;
            }
            st.completion_tokens += 1;
            let (r, stop) = f.push(ch.as_bytes());
            if !r.is_empty() && !sink(&r) {
                break;
            }
            if stop {
                st.finish = "stop".into();
                break;
            }
        }
        if st.finish != "stop" && (st.completion_tokens as usize) >= text.split_inclusive(' ').count() {
            st.finish = "stop".into();
        }
        let r = f.finish();
        if !r.is_empty() {
            sink(&r);
        }
        st.gen_ms = t0.elapsed().as_secs_f64() * 1000.0;
        Ok(st)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(stops: &[&str], pieces: &[&[u8]]) -> (String, bool) {
        let mut f = StopFilter::new(&stops.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let mut out = String::new();
        for p in pieces {
            let (r, s) = f.push(p);
            out.push_str(&r);
            if s {
                return (out, true);
            }
        }
        out.push_str(&f.finish());
        (out, false)
    }

    #[test]
    fn stop_word_across_tokens() {
        let (o, s) = run(&["</end>"], &[b"Hallo ", b"Welt</", b"end> rest"]);
        assert_eq!(o, "Hallo Welt");
        assert!(s);
    }

    #[test]
    fn utf8_split_arabic() {
        let a = "مرحبا".as_bytes();
        let (o, _) = run(&[], &[&a[..1], &a[1..5], &a[5..]]);
        assert_eq!(o, "مرحبا");
    }

    #[test]
    fn no_stop_passes_all() {
        let (o, s) = run(&["XYZ"], &[b"ab", b"XY", b"c"]);
        assert_eq!(o, "abXYc");
        assert!(!s);
    }

    #[test]
    fn mock_json() {
        let mut m = Mock { n_ctx: 512 };
        let pr = m.chat_prompt(&[Msg { role: "user".into(), content: "hi".into(), images: vec![] }], true).unwrap();
        let mut out = String::new();
        let p = GenParams { grammar: Some(JSON_GBNF.into()), ..Default::default() };
        m.generate(&pr, &p, &mut |s| {
            out.push_str(s);
            true
        })
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["answer"], "echo: hi");
    }
}
