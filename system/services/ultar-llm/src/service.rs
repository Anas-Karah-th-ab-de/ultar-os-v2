//! Arbeits-Thread: lädt das Modell bei der ersten Anfrage, arbeitet Anfragen der Reihe nach ab,
//! entlädt bei Leerlauf, Speicherdruck, leerem Akku oder auf Anweisung. Schützt Gerät und Oberfläche.

use crate::config::{self, Config, ModelFile};
use crate::engine::{GenParams, ModelInfo, Msg, Session, Stats};
use crate::platform::{self, Profile};
use serde::Serialize;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum Input {
    Chat { msgs: Vec<Msg>, json_only: bool },
    Text(String),
}

pub struct Job {
    pub model: Option<String>,
    pub input: Input,
    pub params: GenParams,
    pub events: Sender<Event>,
}

pub enum Event {
    Started { model: String },
    Text(String),
    Done(Stats),
    Error { code: u16, msg: String },
}

enum Cmd {
    Job(Job),
    Unload(Sender<()>),
    Load(Option<String>, Sender<Result<ModelInfo, String>>),
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct State {
    /// idle | loading | ready | busy
    pub state: String,
    pub model: Option<ModelInfo>,
    pub queued: usize,
    pub requests: u64,
    pub last: Option<LastRun>,
    pub last_error: Option<String>,
    pub unload_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LastRun {
    pub model: String,
    pub prompt_tokens: u32,
    pub cached_tokens: u32,
    pub completion_tokens: u32,
    pub prompt_tps: f64,
    pub gen_tps: f64,
}

#[derive(Clone)]
pub struct Handle {
    tx: Sender<Cmd>,
    pub state: Arc<Mutex<State>>,
    pub cfg: Arc<Config>,
    pub profile: Arc<Profile>,
}

/// Warum gerade keine Anfrage angenommen wird (Akku, Hitze) – sonst None.
pub fn gate(c: &Config) -> Option<String> {
    if c.min_battery > 0 {
        if let Some((pct, charging)) = platform::battery() {
            if !charging && pct < c.min_battery {
                return Some(format!("Akku zu niedrig ({pct} %)"));
            }
        }
    }
    if c.max_temp_c > 0.0 {
        if let Some(t) = platform::cpu_temp() {
            if t >= c.max_temp_c {
                return Some(format!("Gerät zu warm ({t:.0} °C)"));
            }
        }
    }
    None
}

impl Handle {
    pub fn start(cfg: Config, profile: Profile) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(State { state: "idle".into(), ..Default::default() }));
        let h = Handle { tx, state: state.clone(), cfg: Arc::new(cfg), profile: Arc::new(profile) };
        let (c, p) = (h.cfg.clone(), h.profile.clone());
        std::thread::Builder::new().name("llm-worker".into()).stack_size(8 << 20).spawn(move || worker(rx, c, p, state)).expect("Arbeits-Thread");
        if h.cfg.preload {
            let (t, _r) = mpsc::channel();
            let _ = h.tx.send(Cmd::Load(None, t));
        }
        h
    }

    pub fn submit(&self, job: Job) {
        self.state.lock().unwrap().queued += 1;
        if let Err(mpsc::SendError(Cmd::Job(j))) = self.tx.send(Cmd::Job(job)) {
            let _ = j.events.send(Event::Error { code: 500, msg: "Dienst beendet".into() });
        }
    }

    /// Modell freigeben; kehrt zurück, wenn der Speicher frei ist (oder nach 30 s, falls gerade gerechnet wird).
    pub fn unload(&self) {
        let (t, r) = mpsc::channel();
        if self.tx.send(Cmd::Unload(t)).is_ok() {
            let _ = r.recv_timeout(Duration::from_secs(30));
        }
    }

    pub fn load(&self, model: Option<String>) -> Result<ModelInfo, String> {
        let (t, r) = mpsc::channel();
        self.tx.send(Cmd::Load(model, t)).map_err(|_| "Dienst beendet".to_string())?;
        r.recv().map_err(|_| "Dienst beendet".to_string())?
    }

    pub fn models(&self) -> Vec<ModelFile> {
        let mut v = config::list_models(&self.cfg);
        if self.cfg.model.starts_with("mock:") {
            v.insert(0, ModelFile { id: "mock".into(), path: self.cfg.model.clone(), bytes: 0 });
        }
        v
    }
}

/// Modell zur Anfrage finden: Kennung oder Dateipfad; "", "local", "default" und Unbekanntes → Standard.
fn resolve(c: &Config, p: &Profile, want: Option<&str>) -> Result<ModelFile, String> {
    if let Some(w) = want.filter(|w| !w.is_empty() && !matches!(*w, "local" | "default" | "ultar")) {
        if c.model.starts_with("mock:") {
            return Ok(ModelFile { id: "mock".into(), path: c.model.clone(), bytes: 0 });
        }
        if let Some(m) = config::list_models(c).into_iter().find(|m| m.id == w || m.path == w) {
            return Ok(m);
        }
    }
    config::pick_model(c, p.mem_total).ok_or_else(|| format!("kein Modell gefunden (.gguf in {})", c.model_dirs.join(", ")))
}

fn open(c: &Config, m: &ModelFile) -> Result<Box<dyn Session>, String> {
    if m.path.starts_with("mock:") {
        return Ok(Box::new(crate::engine::Mock { n_ctx: c.ctx }));
    }
    // Speicher prüfen: Modell (per mmap) + Kontext + Reserve müssen hineinpassen
    let avail = platform::mem_available();
    let need = m.bytes + c.mem_reserve_mb * 1024 * 1024;
    if avail > 0 && need > avail + platform::meminfo("Cached") / 2 {
        return Err(format!("zu wenig freier Speicher: Modell {} MB + Reserve {} MB, frei {} MB", m.bytes >> 20, c.mem_reserve_mb, avail >> 20));
    }
    #[cfg(feature = "llama")]
    {
        crate::llama::open(c, &m.id, &m.path)
    }
    #[cfg(not(feature = "llama"))]
    {
        Err("ohne Rechenkern gebaut (Feature llama)".into())
    }
}

fn worker(rx: Receiver<Cmd>, c: Arc<Config>, p: Arc<Profile>, state: Arc<Mutex<State>>) {
    platform::renice(c.nice);
    if c.big_cores_only {
        platform::pin_to(&p.big_cpus);
    }
    let mut sess: Option<(Box<dyn Session>, ModelFile)> = None;
    let mut last_use = Instant::now();
    let set = |f: &dyn Fn(&mut State)| f(&mut state.lock().unwrap());
    loop {
        // Während ein Modell geladen ist: alle 5 s Leerlauf/Speicherdruck/Akku prüfen
        let cmd = if sess.is_some() {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        let Some(cmd) = cmd else {
            let idle = last_use.elapsed().as_secs();
            let reason = if c.idle_unload_secs > 0 && idle >= c.idle_unload_secs {
                Some(format!("Leerlauf {idle} s"))
            } else if c.unload_pressure > 0.0 && idle >= 10 && platform::mem_pressure().map(|v| v >= c.unload_pressure).unwrap_or(false) {
                Some("Speicherdruck".to_string())
            } else {
                gate(&c)
            };
            if let Some(r) = reason {
                sess = None;
                set(&|s| {
                    s.state = "idle".into();
                    s.model = None;
                    s.unload_reason = Some(r.clone());
                });
            }
            continue;
        };
        match cmd {
            Cmd::Unload(done) => {
                sess = None;
                let _ = done.send(());
                set(&|s| {
                    s.state = "idle".into();
                    s.model = None;
                    s.unload_reason = Some("auf Anweisung".into());
                });
            }
            Cmd::Load(want, reply) => {
                let r = ensure(&c, &p, &mut sess, want.as_deref(), &state);
                let _ = reply.send(r.map(|_| sess.as_ref().unwrap().0.info()));
                last_use = Instant::now();
            }
            Cmd::Job(job) => {
                set(&|s| s.queued = s.queued.saturating_sub(1));
                if let Some(why) = gate(&c) {
                    sess = None;
                    set(&|s| {
                        s.state = "idle".into();
                        s.model = None;
                    });
                    let _ = job.events.send(Event::Error { code: 503, msg: why });
                    continue;
                }
                if let Err(e) = ensure(&c, &p, &mut sess, job.model.as_deref(), &state) {
                    set(&|s| s.last_error = Some(e.clone()));
                    let _ = job.events.send(Event::Error { code: 503, msg: e });
                    continue;
                }
                let (s, mf) = sess.as_mut().unwrap();
                let prompt = match &job.input {
                    Input::Chat { msgs, json_only } => s.chat_prompt(msgs, *json_only),
                    Input::Text(t) => Ok(t.clone()),
                };
                let prompt = match prompt {
                    Ok(x) => x,
                    Err(e) => {
                        let _ = job.events.send(Event::Error { code: 400, msg: e });
                        continue;
                    }
                };
                // Bilder der Nachrichten (in Reihenfolge der Marken im Prompt)
                let images: Vec<String> = match &job.input {
                    Input::Chat { msgs, .. } => msgs.iter().flat_map(|m| m.images.iter().cloned()).collect(),
                    Input::Text(_) => Vec::new(),
                };
                if !images.is_empty() && !s.vision() {
                    let _ = job.events.send(Event::Error { code: 400, msg: format!("model {} cannot see images – install a vision model with its mmproj file (ultar-llm recommend)", mf.id) });
                    continue;
                }
                set(&|s| s.state = "busy".into());
                let _ = job.events.send(Event::Started { model: mf.id.clone() });
                let ev = job.events.clone();
                let r = if images.is_empty() {
                    s.generate(&prompt, &job.params, &mut |t| ev.send(Event::Text(t.to_string())).is_ok())
                } else {
                    s.generate_mm(&prompt, &images, &job.params, &mut |t| ev.send(Event::Text(t.to_string())).is_ok())
                };
                let id = mf.id.clone();
                match r {
                    Ok(st) => {
                        let lr = LastRun {
                            model: id,
                            prompt_tokens: st.prompt_tokens,
                            cached_tokens: st.cached_tokens,
                            completion_tokens: st.completion_tokens,
                            prompt_tps: (st.prompt_tps() * 10.0).round() / 10.0,
                            gen_tps: (st.gen_tps() * 10.0).round() / 10.0,
                        };
                        set(&|s| {
                            s.last = Some(lr.clone());
                            s.requests += 1;
                        });
                        let _ = job.events.send(Event::Done(st));
                    }
                    Err(e) => {
                        set(&|s| s.last_error = Some(e.clone()));
                        let code = if e.starts_with("Prompt zu lang") { 400 } else { 500 };
                        let _ = job.events.send(Event::Error { code, msg: e });
                    }
                }
                set(&|s| s.state = "ready".into());
                last_use = Instant::now();
            }
        }
    }
}

fn ensure(c: &Config, p: &Profile, sess: &mut Option<(Box<dyn Session>, ModelFile)>, want: Option<&str>, state: &Mutex<State>) -> Result<(), String> {
    let m = resolve(c, p, want)?;
    if let Some((_, cur)) = sess {
        if cur.path == m.path {
            return Ok(());
        }
    }
    *sess = None; // altes Modell zuerst freigeben (Speicher)
    {
        let mut s = state.lock().unwrap();
        s.state = "loading".into();
        s.model = None;
    }
    match open(c, &m) {
        Ok(x) => {
            let info = x.info();
            *sess = Some((x, m));
            let mut s = state.lock().unwrap();
            s.state = "ready".into();
            s.model = Some(info);
            s.unload_reason = None;
            Ok(())
        }
        Err(e) => {
            let mut s = state.lock().unwrap();
            s.state = "idle".into();
            s.last_error = Some(e.clone());
            Err(e)
        }
    }
}
