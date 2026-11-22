//! ultar-llm – Dienst und Befehlszeile.
//!
//!   ultar-llm [serve] [--config DATEI] [--model GGUF|mock:] [--listen IP:PORT|""] [--socket PFAD|""]
//!   ultar-llm info                      Gerät + wirksame Einstellungen
//!   ultar-llm status | models | unload | load [MODELL]
//!   ultar-llm ask "Frage" [--json] [--model ID] [--system TEXT] [--image BILD]
//!   ultar-llm chat                      Gespräch im Terminal
//!   ultar-llm bench [--model GGUF] [--tokens N]   misst Tempo je Thread-Zahl und empfiehlt Einstellungen
//!   ultar-llm catalog                   geprüfte Modelle (Gespräch + Bilder), Empfehlung für dieses Gerät
//!   ultar-llm recommend [--json]        welches Modell für dieses Gerät
//!   ultar-llm pull <id|id@ollama|hf:repo/datei.gguf[+mmproj.gguf]|ollama:name:tag> [--dir ORDNER]

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use ultar_llm::{client, config::Config, platform, service::Handle};

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn load_cfg(args: &[String]) -> Result<(Config, platform::Profile), String> {
    let p = platform::detect();
    let mut c = Config::load(&p, opt(args, "--config").as_deref().map(std::path::Path::new))?;
    if let Some(m) = opt(args, "--model") {
        c.model = m;
    }
    if let Some(l) = opt(args, "--listen") {
        c.listen = l;
    }
    if let Some(s) = opt(args, "--socket") {
        c.socket = s;
    }
    if let Some(t) = opt(args, "--threads").and_then(|t| t.parse().ok()) {
        c.threads = t;
    }
    if let Some(t) = opt(args, "--ctx").and_then(|t| t.parse().ok()) {
        c.ctx = t;
    }
    Ok((c.resolved(&p), p))
}

fn endpoint(args: &[String]) -> String {
    opt(args, "--endpoint").unwrap_or_else(client::default_endpoint)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().filter(|a| !a.starts_with("--") || matches!(a.as_str(), "--version" | "--help")).cloned().unwrap_or_else(|| "serve".into());
    let r = match cmd.as_str() {
        "serve" => serve(&args),
        "info" => load_cfg(&args).map(|(c, p)| {
            let chosen = ultar_llm::config::pick_model(&c, p.mem_total);
            println!("{}", serde_json::to_string_pretty(&json!({"profile": p, "config": c, "model": chosen, "models": ultar_llm::config::list_models(&c)})).unwrap());
        }),
        "catalog" | "recommend" => {
            let p = platform::detect();
            let r = ultar_llm::models::recommend(&p);
            if flag(&args, "--json") {
                println!("{}", serde_json::to_string_pretty(&json!({"recommend": r, "catalog": ultar_llm::models::CATALOG})).unwrap());
            } else {
                println!("Gerät: {} · {} · GPU {}", r.why, r.cpu, r.gpu.clone().unwrap_or_else(|| "–".into()));
                println!("Empfohlen: Gespräch/Werkzeuge = {}, Bilder = {}", r.chat, r.vision.unwrap_or("– (zu wenig Speicher)"));
                if cmd == "catalog" {
                    println!();
                    for e in ultar_llm::models::CATALOG {
                        let mark = if e.id == r.chat || Some(e.id) == r.vision { "★" } else { " " };
                        println!("{mark} {:<16} {:<26} {:>6} MB Datei, {:>6} MB RAM  {}  HF {}{}", e.id, e.name, e.size_mb, e.ram_mb, e.langs, e.hf_repo, if e.ollama.is_empty() { String::new() } else { format!(" · Ollama {}", e.ollama) });
                    }
                    println!("\nLaden: ultar-llm pull <id>   (Ollama: <id>@ollama)");
                }
            }
            Ok(())
        }
        "pull" => {
            let spec = args.get(1).cloned().unwrap_or_default();
            if spec.is_empty() {
                Err("ultar-llm pull <id|id@ollama|hf:…|ollama:…>".into())
            } else {
                let dir = opt(&args, "--dir").unwrap_or_else(|| load_cfg(&args).ok().and_then(|(c, _)| c.model_dirs.first().cloned()).unwrap_or_else(|| "/data/ultar/models".into()));
                let mut last = String::new();
                let r = ultar_llm::models::pull(&spec, &dir, &mut |label, done, total| {
                    // eine Zeile je Fortschritt (die Oberfläche liest sie mit)
                    let line = if total > 0 { format!("{label}: {} / {} MB ({}%)", done >> 20, total >> 20, done * 100 / total.max(1)) } else { format!("{label}: {} MB", done >> 20) };
                    if line != last {
                        println!("{line}");
                        last = line;
                    }
                });
                r.map(|p| println!("fertig: {}{}", p.model, p.mmproj.map(|m| format!(" + {m}")).unwrap_or_default()))
            }
        }
        "status" => client::request(&endpoint(&args), "GET", "/ultar/status", None, 10).map(|v| println!("{}", serde_json::to_string_pretty(&v).unwrap())),
        "models" => client::request(&endpoint(&args), "GET", "/v1/models", None, 10).map(|v| {
            for m in v["data"].as_array().cloned().unwrap_or_default() {
                println!("{:<40} {:>7} MB  {}", m["id"].as_str().unwrap_or(""), m["meta"]["bytes"].as_u64().unwrap_or(0) >> 20, m["meta"]["path"].as_str().unwrap_or(""));
            }
        }),
        "unload" => client::request(&endpoint(&args), "POST", "/ultar/unload", Some(&json!({})), 30).map(|_| println!("entladen")),
        "load" => client::request(&endpoint(&args), "POST", "/ultar/load", Some(&json!({"model": args.get(1)})), 600).map(|v| println!("{}", serde_json::to_string_pretty(&v["model"]).unwrap())),
        "ask" => ask(&args),
        "chat" => chat(&args),
        "bench" => bench(&args),
        "--version" | "version" => {
            println!("ultar-llm {} ({})", env!("CARGO_PKG_VERSION"), if cfg!(feature = "llama") { "llama.cpp" } else { "ohne Rechenkern" });
            Ok(())
        }
        "help" | "--help" | "-h" => {
            println!("{}", include_str!("main.rs").lines().take(9).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
            Ok(())
        }
        other => Err(format!("unbekannter Befehl: {other} (help)")),
    };
    if let Err(e) = r {
        eprintln!("ultar-llm: {e}");
        std::process::exit(1);
    }
}

fn serve(args: &[String]) -> Result<(), String> {
    let (c, p) = load_cfg(args)?;
    eprintln!(
        "ultar-llm {}: {:?}, {} CPUs ({} schnelle), {} MB RAM, SIMD {}, Threads {}/{}, Kontext {}",
        env!("CARGO_PKG_VERSION"),
        p.kind,
        p.cpus,
        p.big_cpus.len(),
        p.mem_total >> 20,
        p.simd,
        c.threads,
        c.threads_batch,
        c.ctx
    );
    match ultar_llm::config::pick_model(&c, p.mem_total) {
        Some(m) => eprintln!("ultar-llm: Standardmodell {} ({} MB) – wird bei der ersten Anfrage geladen", m.path, m.bytes >> 20),
        None => eprintln!("ultar-llm: noch kein Modell (.gguf in {})", c.model_dirs.join(", ")),
    }
    // Abbruch-Signale: Socket aufräumen
    let sock = c.socket.clone();
    unsafe {
        SOCK.get_or_init(|| sock);
        libc::signal(libc::SIGTERM, on_term as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    ultar_llm::http::run(Handle::start(c, p))
}

static SOCK: std::sync::OnceLock<String> = std::sync::OnceLock::new();

extern "C" fn on_term(_: libc::c_int) {
    if let Some(s) = SOCK.get() {
        if !s.is_empty() {
            if let Ok(cs) = std::ffi::CString::new(s.as_str()) {
                unsafe { libc::unlink(cs.as_ptr()) };
            }
        }
    }
    unsafe { libc::_exit(0) };
}

fn ask(args: &[String]) -> Result<(), String> {
    let q: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with("--")).collect();
    // Werte von Optionen nicht als Frage werten
    let skip: Vec<String> = ["--model", "--system", "--endpoint", "--image"].iter().filter_map(|o| opt(args, o)).collect();
    let q: Vec<&str> = q.iter().map(|s| s.as_str()).filter(|s| !skip.iter().any(|k| k == s)).collect();
    let mut question = q.join(" ");
    if question.is_empty() || question == "-" {
        question.clear();
        std::io::stdin().lock().read_line(&mut question).map_err(|e| e.to_string())?;
    }
    let mut msgs = vec![];
    if let Some(s) = opt(args, "--system") {
        msgs.push(json!({"role": "system", "content": s}));
    }
    // --image BILD: Frage zu einem Bild (braucht ein Bild-Modell mit mmproj)
    let image = opt(args, "--image");
    match &image {
        Some(img) => {
            let path = std::fs::canonicalize(img).map_err(|e| format!("{img}: {e}"))?;
            msgs.push(json!({"role": "user", "content": [{"type": "text", "text": question.trim()}, {"type": "image_url", "image_url": {"url": format!("file://{}", path.display())}}]}));
        }
        None => msgs.push(json!({"role": "user", "content": question.trim()})),
    }
    let default_model = if image.is_some() { "vision" } else { "local" };
    let mut body = json!({"model": opt(args, "--model").unwrap_or_else(|| default_model.into()), "messages": msgs});
    if flag(args, "--json") {
        body["response_format"] = json!({"type": "json_object"});
        body["temperature"] = json!(0.2);
    }
    let t0 = std::time::Instant::now();
    let (_, usage) = client::chat_stream(&endpoint(args), &body, &mut |t| {
        print!("{t}");
        let _ = std::io::stdout().flush();
    })?;
    println!();
    if flag(args, "--stats") {
        eprintln!("[{} Token in {:.1} s]", usage["completion_tokens"], t0.elapsed().as_secs_f64());
    }
    Ok(())
}

fn chat(args: &[String]) -> Result<(), String> {
    let ep = endpoint(args);
    let mut hist: Vec<Value> = vec![];
    if let Some(s) = opt(args, "--system") {
        hist.push(json!({"role": "system", "content": s}));
    }
    eprintln!("Ultar KI – lokal auf diesem Gerät. Leere Zeile oder /ende beendet, /neu leert den Verlauf.");
    let stdin = std::io::stdin();
    loop {
        print!("\n> ");
        let _ = std::io::stdout().flush();
        let mut l = String::new();
        if stdin.lock().read_line(&mut l).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let l = l.trim();
        match l {
            "" | "/ende" | "/exit" => break,
            "/neu" | "/new" => {
                hist.retain(|m| m["role"] == "system");
                continue;
            }
            _ => {}
        }
        hist.push(json!({"role": "user", "content": l}));
        let (text, _) = client::chat_stream(&ep, &json!({"model": opt(args, "--model").unwrap_or_else(|| "local".into()), "messages": hist}), &mut |t| {
            print!("{t}");
            let _ = std::io::stdout().flush();
        })?;
        println!();
        hist.push(json!({"role": "assistant", "content": text}));
    }
    Ok(())
}

#[cfg(feature = "llama")]
fn bench(args: &[String]) -> Result<(), String> {
    use ultar_llm::engine::{GenParams, Msg};
    let (mut c, p) = load_cfg(args)?;
    let m = ultar_llm::config::pick_model(&c, p.mem_total).ok_or("kein Modell")?;
    let n: u32 = opt(args, "--tokens").and_then(|t| t.parse().ok()).unwrap_or(64);
    let mut cands: Vec<i32> = vec![p.big_cpus.len() as i32, 2, 4, 6, 8, p.cores as i32, p.cpus as i32];
    cands.retain(|&t| t >= 1 && t as usize <= p.cpus);
    cands.sort();
    cands.dedup();
    if let Some(t) = opt(args, "--threads").and_then(|t| t.parse().ok()) {
        cands = vec![t];
    }
    println!("Modell {} ({} MB), {:?}, {} CPUs, schnelle: {:?}, SIMD {}", m.path, m.bytes >> 20, p.kind, p.cpus, p.big_cpus, p.simd);
    let msgs = [Msg { role: "user".into(), content: "Erkläre in drei Sätzen, wie ein Akku funktioniert. / Explain in three sentences how a battery works.".into(), images: vec![] }];
    let mut best = (0.0f64, 0i32);
    for t in cands {
        c.threads = t;
        c.threads_batch = p.cpus.min(8) as i32;
        let mut s = ultar_llm::llama::open(&c, &m.id, &m.path)?;
        let prompt = s.chat_prompt(&msgs, false)?;
        let gp = GenParams { max_tokens: n, temperature: 0.0, ..Default::default() };
        let st = s.generate(&prompt, &gp, &mut |_| true)?;
        let temp = platform::cpu_temp().map(|t| format!("{t:.0} °C")).unwrap_or_default();
        println!(
            "Threads {t:>2}: Einlesen {:>7.1} Tok/s  Erzeugen {:>6.2} Tok/s  ({} Token)  {temp}",
            st.prompt_tps(),
            st.gen_tps(),
            st.completion_tokens
        );
        if st.gen_tps() > best.0 {
            best = (st.gen_tps(), t);
        }
        drop(s);
        std::thread::sleep(std::time::Duration::from_secs(2)); // abkühlen
    }
    println!("\nEmpfehlung für /data/ultar/llm.toml:\nthreads = {}", best.1);
    Ok(())
}

#[cfg(not(feature = "llama"))]
fn bench(_: &[String]) -> Result<(), String> {
    Err("ohne Rechenkern gebaut".into())
}
