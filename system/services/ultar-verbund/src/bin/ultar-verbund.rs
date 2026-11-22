//! ultar-verbund – Befehlszeile und Dienst des Ultar-Verbunds.

use ultar_sprache::{eprintln, println};
use std::io::{BufRead, Read};
use std::sync::Arc;
use std::time::Duration;
use ultar_verbund::config::{Config, Rolle};
use ultar_verbund::dienst::{self, KoppelCode};
use ultar_verbund::entdeckung::{self, Gesehen};
use ultar_verbund::ident::{self, Ident};
use ultar_verbund::planer::{Knoten, Planer};
use ultar_verbund::proto::{Aufgabe, Nachricht};
use ultar_verbund::relay::{Anmeldung, Relais};
use ultar_verbund::*;
use base64::Engine;

const HILFE: &str = ultar_sprache::tabellen::VERBUND_HILFE_DE;

fn main() {
    let args: Vec<String> = ultar_sprache::argumente();
    let dir = datenordner();
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = match a.as_slice() {
        ["dienst"] => dienst::starten(dir).map_err(|e| e.to_string()),
        ["an"] => schalten(&dir, true),
        ["aus"] => schalten(&dir, false),
        ["status"] | [] => status(&dir),
        ["geraete"] | ["geräte"] => geraete(&dir),
        ["koppeln"] => koppeln_anbieten(&dir, Rolle::Voll),
        ["koppeln", "--rolle", r] => koppeln_anbieten(&dir, if *r == "rechnen" { Rolle::Rechnen } else { Rolle::Voll }),
        ["koppeln", ziel, code] => koppeln_mit(&dir, ziel, code),
        ["entkoppeln", ziel] => entkoppeln(&dir, ziel),
        ["konto", rest @ ..] => konto(&dir, rest),
        ["name", n] => aendern(&dir, |c| c.name = n.to_string()),
        ["relais", u] => aendern(&dir, |c| c.relais = u.to_string()),
        ["kerne", n] => aendern(&dir, |c| c.max_kerne = n.parse().unwrap_or(0)),
        ["ziel", z] => aendern(&dir, |c| {
            if !c.ziele.iter().any(|x| x == z) {
                c.ziele.push(z.to_string())
            }
        }),
        ["erlauben", was, wert] => {
            let an = *wert == "an";
            match *was {
                "steuern" => aendern(&dir, |c| c.steuern_erlauben = an),
                "rechnen" => aendern(&dir, |c| c.rechnen_erlauben = an),
                "bildschirm" => aendern(&dir, |c| c.bildschirm_erlauben = an),
                "ton" => aendern(&dir, |c| c.ton_erlauben = an),
                "dateien" => aendern(&dir, |c| c.dateien_erlauben = an),
                _ => Err("steuern | rechnen | bildschirm | ton | dateien".into()),
            }
        }
        ["ausfuehren", ziel, "--", cmd @ ..] if !cmd.is_empty() => ausfuehren(&dir, ziel, cmd),
        ["verteilt", rest @ ..] => verteilt(&dir, rest),
        ["anzeigen", ziel, rest @ ..] => anzeigen(&dir, ziel, rest),
        ["foto", ziel, datei] => foto(&dir, ziel, datei),
        ["bildschirm", ziel, rest @ ..] if !rest.is_empty() => {
            let zeile = rest.join(" ");
            frage(&dir, ziel, Nachricht::Aktion { zeile }).and_then(antwort_zeigen)
        }
        ["fern", ziel, zeilen @ ..] if !zeilen.is_empty() => fern(&dir, ziel, zeilen),
        ["senden", ziel, datei, rest @ ..] => senden(&dir, ziel, datei, !rest.contains(&"--nicht-oeffnen")),
        ["weitergeben", ziel] => frage(&dir, ziel, Nachricht::Ansehen).and_then(antwort_zeigen),
        ["weitergeben", ziel, datei] => senden(&dir, ziel, datei, true),
        ["hoeren", ziel, rest @ ..] => hoeren(&dir, ziel, rest.first().and_then(|s| s.parse().ok()).unwrap_or(0)),
        ["sprechen", ziel, rest @ ..] => sprechen(&dir, ziel, rest.first().and_then(|s| s.parse().ok()).unwrap_or(0)),
        ["mining", rest @ ..] => mining(&dir, rest),
        ["bench", rest @ ..] => bench(&dir, rest),
        ["arbeit", i, iter] => {
            println!("{}", arbeit(i, iter.parse().unwrap_or(100_000)));
            Ok(())
        }
        ["--version"] => {
            println!("ultar-verbund {VERSION}");
            Ok(())
        }
        _ => {
            println!("{HILFE}");
            Ok(())
        }
    };
    if let Err(e) = r {
        eprintln!("ultar-verbund: {e}");
        std::process::exit(1);
    }
}

type R = Result<(), String>;

/// deterministische Rechenarbeit: SHA-256-Kette – Ergebnis überall gleich und prüfbar
pub fn arbeit(i: &str, iter: u64) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::digest(format!("ultar-verbund/{i}").as_bytes());
    for _ in 0..iter {
        h = Sha256::digest(h);
    }
    ident::hex(&h[..8])
}

fn aendern(dir: &std::path::Path, f: impl FnOnce(&mut Config)) -> R {
    let mut c = Config::datei(dir);
    f(&mut c);
    c.speichern(dir).map_err(|e| e.to_string())?;
    println!("gespeichert");
    Ok(())
}

fn schalten(dir: &std::path::Path, an: bool) -> R {
    // Kern-Schalter (Ultar-Kern) mitsetzen
    if std::path::Path::new("/proc/ultar/verbund").exists() {
        let _ = std::fs::write("/proc/ultar/verbund", if an { "an" } else { "aus" });
    }
    let mut c = Config::datei(dir);
    c.aktiv = an;
    c.speichern(dir).map_err(|e| e.to_string())?;
    Ident::laden(dir).map_err(|e| e.to_string())?;
    println!("Ultar-Verbund {}", if an { "AN – Geräte im Netz werden gefunden, gekoppelte Geräte helfen sich gegenseitig" } else { "AUS – dieses Gerät ist für andere unsichtbar" });
    Ok(())
}

fn status(dir: &std::path::Path) -> R {
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let c = Config::laden(dir);
    let z = dienst::zustand_lesen(dir);
    let laeuft = jetzt_ms().saturating_sub(z.zeit_ms) < 5000;
    println!("Gerät      {} „{}“ ({})", ich.kennung(), c.name, ressourcen::ausgabe());
    println!("Verbund    {}", if c.aktiv { "AN" } else { "AUS" });
    println!("Dienst     {}", if laeuft { "läuft" } else { "läuft nicht (ultar-verbund dienst)" });
    println!("Kerne      {} (max {})", ressourcen::kerne(), c.kerne());
    println!("Relais     {} {}", c.relais, if c.token.is_empty() { "(kein Konto)" } else if z.relais { "verbunden" } else { "getrennt" });
    println!("Erlaubt    steuern={} rechnen={}", c.steuern_erlauben, c.rechnen_erlauben);
    println!("Gekoppelt  {} Geräte, erledigte Aufgaben {}", c.partner.len(), z.erledigt);
    Ok(())
}

fn geraete_finden(dir: &std::path::Path, c: &Config, ich: &Ident) -> Vec<Gesehen> {
    let z = dienst::zustand_lesen(dir);
    if jetzt_ms().saturating_sub(z.zeit_ms) < 5000 {
        return z.geraete;
    }
    // kein Dienst: selbst 2,5 s lauschen
    let t: entdeckung::Tabelle = Default::default();
    if let Ok(u) = entdeckung::socket(c.udp_port) {
        let start = std::time::Instant::now();
        let mut b = [0u8; 2048];
        while start.elapsed() < Duration::from_millis(2500) {
            if let Ok((n, von)) = u.recv_from(&mut b) {
                entdeckung::verarbeiten(&t, &ich.pk_hex(), &b[..n], von, |pk| c.partner(pk).is_some());
            }
        }
    }
    let v: Vec<Gesehen> = t.lock().unwrap().values().cloned().collect();
    v
}

fn geraete(dir: &std::path::Path) -> R {
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let c = Config::laden(dir);
    let v = geraete_finden(dir, &c, &ich);
    if v.is_empty() {
        println!("keine Geräte gefunden{}", if c.aktiv { "" } else { " (Verbund ist AUS)" });
    }
    for g in &v {
        println!(
            "{}  {:<20} {:<8} {:>3}/{:<3} Kerne frei  {:<22} {}",
            g.kennung,
            g.name,
            g.ausgabe,
            g.frei,
            g.kerne,
            g.adresse,
            if g.vertraut { "gekoppelt" } else { "–" }
        );
    }
    for p in &c.partner {
        if !v.iter().any(|g| g.pk == p.pk) {
            println!("{}  {:<20} (nicht erreichbar, gekoppelt über {})", p.kennung, p.name, p.quelle);
        }
    }
    Ok(())
}

fn ziel_suchen(dir: &std::path::Path, c: &Config, ich: &Ident, ziel: &str) -> Result<Gesehen, String> {
    if let Ok(a) = ziel.parse::<std::net::SocketAddr>() {
        return Ok(Gesehen { kennung: String::new(), pk: String::new(), name: ziel.into(), ausgabe: String::new(), adresse: a.to_string(), kerne: 1, frei: 1, zuletzt_ms: 0, vertraut: false, weg: "netz".into() });
    }
    let v = geraete_finden(dir, c, ich);
    v.into_iter()
        .find(|g| g.kennung.eq_ignore_ascii_case(ziel) || g.name == ziel || g.pk == ziel)
        .ok_or_else(|| format!("Gerät „{ziel}“ nicht gefunden (ultar-verbund geraete)"))
}

fn relais_client(c: &Config, ich: &Ident) -> Option<Arc<Relais>> {
    if c.token.is_empty() || c.relais.is_empty() {
        return None;
    }
    let r = Relais::starten(
        Anmeldung { url: c.relais.clone(), token: c.token.clone(), pk: ich.pk_hex(), name: c.name.clone(), ausgabe: ressourcen::ausgabe(), rolle: "client".into(), kerne: 0 },
        None,
        Arc::new(|| 0),
    );
    r.warte_verbunden(Duration::from_secs(5));
    Some(r)
}

fn koppeln_anbieten(dir: &std::path::Path, rolle: Rolle) -> R {
    use rand::Rng;
    let c = Config::laden(dir);
    if !c.aktiv {
        return Err("Verbund ist AUS – zuerst: ultar-verbund an".into());
    }
    let code = std::env::var("ULTAR_VERBUND_CODE").unwrap_or_else(|_| format!("{:06}", rand::thread_rng().gen_range(0..1_000_000)));
    let kc = KoppelCode { code: code.clone(), bis_ms: jetzt_ms() + 300_000, rolle, fehlversuche: 0 };
    let p = dir.join("koppeln.json");
    std::fs::write(&p, serde_json::to_vec(&kc).unwrap()).map_err(|e| e.to_string())?;
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    println!("Kopplungscode: {code}   (5 Minuten gültig)");
    println!("Auf dem anderen Gerät: ultar-verbund koppeln {} {code}", ich.kennung());
    let vorher = c.partner.len();
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_secs(300) {
        std::thread::sleep(Duration::from_millis(300));
        if !p.exists() {
            let n = Config::laden(dir);
            if n.partner.len() > vorher || n.partner.iter().any(|x| x.quelle == "code") {
                if let Some(x) = n.partner.last() {
                    println!("gekoppelt mit „{}“ ({})", x.name, x.kennung);
                }
                return Ok(());
            }
            return Err("Kopplung abgebrochen".into());
        }
    }
    let _ = std::fs::remove_file(&p);
    Err("Zeit abgelaufen".into())
}

fn koppeln_mit(dir: &std::path::Path, ziel: &str, code: &str) -> R {
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let c = Config::laden(dir);
    let g = ziel_suchen(dir, &c, &ich, ziel)?;
    let relais = if g.weg == "relais" { relais_client(&c, &ich) } else { None };
    let mut k = if g.pk.is_empty() {
        // nur Adresse bekannt: Schlüssel wird durch den Code bestätigt
        let a: std::net::SocketAddr = g.adresse.parse().map_err(|_| "Adresse")?;
        let s = std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)).map_err(|e| e.to_string())?;
        kanal::Kanal::verbinden(Box::new(kanal::TcpTransport(s)), &ich).map_err(|e| e.to_string())?
    } else {
        dienst::verbinden(&ich, &g, relais.as_ref()).map_err(|e| e.to_string())?
    };
    let partner_pk = k.partner_pk.clone();
    let mac = dienst::koppel_mac(code, &ich.oeffentlich, &partner_pk);
    k.nachricht_senden(&Nachricht::Koppeln { name: c.name.clone(), mac }).map_err(|e| e.to_string())?;
    match k.nachricht_empfangen().map_err(|e| e.to_string())? {
        Nachricht::KoppelnOk { name, mac } => {
            if mac != dienst::koppel_mac(code, &partner_pk, &ich.oeffentlich) {
                return Err("Antwort nicht echt (Code passt nicht)".into());
            }
            let mut c = Config::datei(dir);
            c.hinzufuegen(config::Partner { kennung: ident::kennung(&partner_pk), pk: ident::hex(&partner_pk), name: name.clone(), rolle: Rolle::Voll, quelle: "code".into() });
            c.speichern(dir).map_err(|e| e.to_string())?;
            println!("gekoppelt mit „{name}“ ({})", ident::kennung(&partner_pk));
            Ok(())
        }
        Nachricht::Fehler { text } => Err(text),
        _ => Err("unerwartete Antwort".into()),
    }
}

fn entkoppeln(dir: &std::path::Path, ziel: &str) -> R {
    let mut c = Config::datei(dir);
    let vor = c.partner.len();
    c.partner.retain(|p| !(p.kennung.eq_ignore_ascii_case(ziel) || p.name == ziel || p.pk == ziel));
    if c.partner.len() == vor {
        return Err("nicht gekoppelt".into());
    }
    c.speichern(dir).map_err(|e| e.to_string())?;
    println!("entkoppelt");
    Ok(())
}

fn konto(dir: &std::path::Path, rest: &[&str]) -> R {
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let mut c = Config::datei(dir);
    match rest {
        ["anmelden", benutzer] => {
            let pw = match std::env::var("ULTAR_PASSWORT") {
                Ok(p) => p,
                Err(_) => {
                    eprint!("Passwort: ");
                    let mut s = String::new();
                    std::io::stdin().lock().read_line(&mut s).map_err(|e| e.to_string())?;
                    s.trim_end().to_string()
                }
            };
            c.token = konto::anmelden(&c, benutzer, &pw)?;
        }
        ["token", t] => c.token = t.to_string(),
        ["abmelden"] => {
            c.token.clear();
            c.partner.retain(|p| p.quelle != "konto");
        }
        ["abgleichen"] => {}
        _ => return Err("konto anmelden <benutzer> | token <t> | abgleichen | abmelden".into()),
    }
    if !c.token.is_empty() {
        let n = konto::abgleichen(&mut c, &ich)?;
        println!("Konto verbunden – {n} weitere Geräte im Konto vertraut");
    }
    c.speichern(dir).map_err(|e| e.to_string())
}

fn ausfuehren(dir: &std::path::Path, ziel: &str, cmd: &[&str]) -> R {
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let c = Config::laden(dir);
    let g = ziel_suchen(dir, &c, &ich, ziel)?;
    let relais = if g.weg == "relais" { relais_client(&c, &ich) } else { None };
    let mut k = if g.pk.is_empty() {
        // nur Adresse angegeben: Gegenstelle muss ein gekoppeltes Gerät sein
        let a: std::net::SocketAddr = g.adresse.parse().map_err(|_| "Adresse")?;
        let s = std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)).map_err(|e| e.to_string())?;
        let k = kanal::Kanal::verbinden(Box::new(kanal::TcpTransport(s)), &ich).map_err(|e| e.to_string())?;
        if c.partner(&ident::hex(&k.partner_pk)).is_none() {
            return Err(format!("{} ist nicht gekoppelt", ident::kennung(&k.partner_pk)));
        }
        k
    } else {
        dienst::verbinden(&ich, &g, relais.as_ref()).map_err(|e| e.to_string())?
    };
    let mut eingabe = String::new();
    if std::env::var("ULTAR_VERBUND_EINGABE").is_ok() {
        std::io::stdin().read_to_string(&mut eingabe).ok();
    }
    let a = Aufgabe { id: format!("s-{}-{}", jetzt_ms(), std::process::id()), befehl: cmd.iter().map(|s| s.to_string()).collect(), eingabe, zeit_ms: 0 };
    k.nachricht_senden(&Nachricht::Steuern(a)).map_err(|e| e.to_string())?;
    k.zeitlimit(Some(Duration::from_secs(8)));
    let antwort = loop {
        match k.nachricht_empfangen().map_err(|e| e.to_string())? {
            Nachricht::Laeuft => continue,
            n => break n,
        }
    };
    match antwort {
        Nachricht::Ergebnis(e) => {
            print!("{}", e.ausgabe);
            eprint!("{}", e.fehler);
            std::process::exit(e.code.clamp(0, 255));
        }
        Nachricht::Fehler { text } => Err(text),
        Nachricht::Belegt => Err("Gerät ist voll ausgelastet".into()),
        _ => Err("unerwartete Antwort".into()),
    }
}

fn knoten(dir: &std::path::Path, c: &Config, ich: &Ident, lokal: bool) -> (Vec<Knoten>, Option<Arc<Relais>>) {
    let mut v = Vec::new();
    if lokal && c.rechnen_erlauben {
        v.push(Knoten::Lokal { kerne: c.kerne(), name: c.name.clone() });
    }
    if !c.aktiv {
        return (v, None);
    }
    let geraete: Vec<Gesehen> = geraete_finden(dir, c, ich).into_iter().filter(|g| c.partner(&g.pk).is_some()).collect();
    let relais = if geraete.iter().any(|g| g.weg == "relais") { relais_client(c, ich) } else { None };
    for g in geraete {
        let kerne = g.frei.max(1);
        v.push(Knoten::Fern { g, kerne });
    }
    (v, relais)
}

fn verteilt(dir: &std::path::Path, rest: &[&str]) -> R {
    let mut n = 0usize;
    let mut zeilen = false;
    let mut lokal = true;
    let mut json = false;
    let mut zeit = 0u64;
    let mut i = 0;
    while i < rest.len() && rest[i] != "--" {
        match rest[i] {
            "-n" => {
                i += 1;
                n = rest.get(i).and_then(|x| x.parse().ok()).ok_or("-n Zahl")?;
            }
            "--zeilen" => zeilen = true,
            "--ohne-lokal" => lokal = false,
            "--json" => json = true,
            "--zeit" => {
                i += 1;
                zeit = rest.get(i).and_then(|x| x.parse().ok()).ok_or("--zeit ms")?;
            }
            x => return Err(format!("unbekannt: {x}")),
        }
        i += 1;
    }
    let cmd: Vec<&str> = rest.get(i + 1..).unwrap_or(&[]).to_vec();
    if cmd.is_empty() {
        return Err("verteilt [-n N] [--zeilen] -- befehl … {}".into());
    }
    let werte: Vec<String> = if zeilen { std::io::stdin().lock().lines().map_while(Result::ok).collect() } else { (0..n.max(1)).map(|x| x.to_string()).collect() };
    let lauf = format!("{:x}{:x}", jetzt_ms(), std::process::id());
    let aufgaben: Vec<Aufgabe> = werte
        .iter()
        .enumerate()
        .map(|(k, w)| Aufgabe { id: format!("{lauf}-{k}"), befehl: cmd.iter().map(|s| s.replace("{}", w)).collect(), eingabe: String::new(), zeit_ms: zeit })
        .collect();
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let c = Config::laden(dir);
    let (kn, relais) = knoten(dir, &c, &ich, lokal);
    if kn.is_empty() {
        return Err("keine Geräte zum Rechnen".into());
    }
    eprintln!("[verbund] {} Aufgaben auf {} Geräte ({} Kerne): {}", aufgaben.len(), kn.len(), kn.iter().map(|k| match k { Knoten::Lokal { kerne, .. } | Knoten::Fern { kerne, .. } => *kerne }).sum::<usize>(), kn.iter().map(|k| k.name()).collect::<Vec<_>>().join(", "));
    let p = Planer::neu(ich, relais, aufgaben);
    let (erg, st) = p.ausfuehren(kn);
    let mut fehl = 0;
    for e in &erg {
        if e.code != 0 {
            fehl += 1;
        }
        if json {
            println!("{}", serde_json::to_string(e).unwrap());
        } else {
            print!("{}", e.ausgabe);
        }
    }
    eprintln!("[verbund] fertig in {} ms – je Gerät: {:?}, wiederholt {}, doppelt gestartet {}, ausgefallen {:?}, hier nachgeholt {}", st.dauer_ms, st.je_knoten, st.wiederholt, st.spekulativ, st.ausgefallen, st.lokal_nachgeholt);
    if fehl > 0 {
        return Err(format!("{fehl} Aufgaben mit Fehlercode"));
    }
    Ok(())
}

fn bench(dir: &std::path::Path, rest: &[&str]) -> R {
    let mut n = 64usize;
    let mut iter = 400_000u64;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "-n" => n = rest.get(i + 1).and_then(|x| x.parse().ok()).unwrap_or(n),
            "-a" => iter = rest.get(i + 1).and_then(|x| x.parse().ok()).unwrap_or(iter),
            _ => {}
        }
        i += 2;
    }
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let c = Config::laden(dir);
    let exe = "ultar-verbund".to_string();
    let mk = |tag: &str| -> Vec<Aufgabe> { (0..n).map(|k| Aufgabe { id: format!("bench-{tag}-{}-{k}", jetzt_ms()), befehl: vec![exe.clone(), "arbeit".into(), k.to_string(), iter.to_string()], eingabe: String::new(), zeit_ms: 0 }).collect() };
    let erwartet: Vec<String> = (0..n).map(|k| arbeit(&k.to_string(), iter)).collect();
    let pruefe = |e: &[proto::Ergebnis]| e.iter().zip(&erwartet).all(|(e, w)| e.ausgabe.trim() == w);

    let (l, _) = knoten(dir, &Config { aktiv: false, ..c.clone() }, &ich, true);
    let p = Planer::neu(ich.clone(), None, mk("hier"));
    let (e1, s1) = p.ausfuehren(l);
    let (kn, relais) = knoten(dir, &c, &ich, true);
    let geraete = kn.len();
    let p = Planer::neu(ich, relais, mk("verbund"));
    let (e2, s2) = p.ausfuehren(kn);
    println!("Aufgaben            {n} × SHA-256-Kette ({iter})");
    println!("nur dieses Gerät    {} ms  {}", s1.dauer_ms, if pruefe(&e1) { "korrekt" } else { "FEHLER" });
    println!("ganzer Verbund      {} ms  {}  ({geraete} Geräte, {:?})", s2.dauer_ms, if pruefe(&e2) { "korrekt" } else { "FEHLER" }, s2.je_knoten);
    if s2.dauer_ms > 0 {
        println!("Beschleunigung      {:.2}×", s1.dauer_ms as f64 / s2.dauer_ms as f64);
    }
    if !pruefe(&e1) || !pruefe(&e2) {
        return Err("Ergebnisse falsch".into());
    }
    Ok(())
}


// ---------------- Bildschirm, Weitergeben, Ton, Mining ----------------

/// Kanal zu einem Gerät (Name, Kennung oder ip:port) – bei bloßer Adresse muss es gekoppelt sein
fn kanal_zu(dir: &std::path::Path, ziel: &str) -> Result<(kanal::Kanal, Gesehen, Option<Arc<Relais>>, Ident), String> {
    let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
    let c = Config::laden(dir);
    let mut g = ziel_suchen(dir, &c, &ich, ziel)?;
    let relais = if g.weg == "relais" { relais_client(&c, &ich) } else { None };
    let k = if g.pk.is_empty() {
        let a: std::net::SocketAddr = g.adresse.parse().map_err(|_| "Adresse")?;
        let s = std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)).map_err(|e| e.to_string())?;
        let k = kanal::Kanal::verbinden(Box::new(kanal::TcpTransport(s)), &ich).map_err(|e| e.to_string())?;
        if c.partner(&ident::hex(&k.partner_pk)).is_none() {
            return Err(format!("{} ist nicht gekoppelt", ident::kennung(&k.partner_pk)));
        }
        g.pk = ident::hex(&k.partner_pk);
        g.kennung = ident::kennung(&k.partner_pk);
        k
    } else {
        dienst::verbinden(&ich, &g, relais.as_ref()).map_err(|e| e.to_string())?
    };
    Ok((k, g, relais, ich))
}

fn frage(dir: &std::path::Path, ziel: &str, n: Nachricht) -> Result<Nachricht, String> {
    let (mut k, ..) = kanal_zu(dir, ziel)?;
    k.zeitlimit(Some(Duration::from_secs(60)));
    k.nachricht_senden(&n).map_err(|e| e.to_string())?;
    k.nachricht_empfangen().map_err(|e| e.to_string())
}

fn antwort_zeigen(n: Nachricht) -> R {
    match n {
        Nachricht::Ok { text } => {
            println!("{text}");
            Ok(())
        }
        Nachricht::Fehler { text } => Err(text),
        Nachricht::MiningStatus { status } => {
            println!("{}", serde_json::to_string_pretty(&status).unwrap());
            Ok(())
        }
        x => Err(format!("unerwartete Antwort: {x:?}")),
    }
}

fn anzeigen(dir: &std::path::Path, ziel: &str, rest: &[&str]) -> R {
    let adresse = rest.iter().position(|x| *x == "--web").and_then(|i| rest.get(i + 1)).copied().unwrap_or("127.0.0.1:0");
    let oeffnen = rest.contains(&"--oeffnen");
    let (k, g, relais, ich) = kanal_zu(dir, ziel)?;
    drop(k);
    let a = web::Ansicht::neu(ich, g.clone(), relais, web::token());
    let url = a.starten(adresse).map_err(|e| format!("{adresse}: {e}"))?;
    // gleich verbinden: die Quelle beginnt zu teilen, das erste Bild steht bereit, wenn der Browser fragt
    if let Err(e) = a.bild(Vec::new(), true) {
        eprintln!("[verbund] {e} – neuer Versuch beim Öffnen");
    }
    println!("{url}");
    eprintln!("[verbund] Ansicht von „{}“ – im Browser öffnen: {url}", g.name);
    if oeffnen {
        // die eigene Oberfläche zeigt die Ansicht im Browser
        bildschirm::an_oberflaeche(&format!("browser {url}"))?;
    }
    loop {
        std::thread::sleep(Duration::from_secs(5));
        let ruhe = jetzt_ms().saturating_sub(a.zuletzt.load(std::sync::atomic::Ordering::Relaxed));
        if oeffnen && ruhe > 600_000 {
            return Ok(()); // 10 min niemand zugesehen
        }
    }
}

fn bild_holen(dir: &std::path::Path, ziel: &str, zeilen: Vec<String>) -> Result<(usize, usize, Vec<u8>), String> {
    let (mut k, ..) = kanal_zu(dir, ziel)?;
    k.zeitlimit(Some(Duration::from_secs(30)));
    k.nachricht_senden(&Nachricht::Teilen).map_err(|e| e.to_string())?;
    if let Nachricht::Fehler { text } = k.nachricht_empfangen().map_err(|e| e.to_string())? {
        return Err(text);
    }
    let mut runde = |eingaben: Vec<String>, voll: bool, warte: u32| -> Result<(usize, usize, Vec<u8>), String> {
        k.nachricht_senden(&Nachricht::Fern { eingaben, warte_ms: warte, voll }).map_err(|e| e.to_string())?;
        match k.nachricht_empfangen().map_err(|e| e.to_string())? {
            Nachricht::Bild { w, h, daten, .. } => Ok((w as usize, h as usize, base64::engine::general_purpose::STANDARD.decode(daten).map_err(|e| e.to_string())?)),
            Nachricht::Fehler { text } => Err(text),
            _ => Err("unerwartet".into()),
        }
    };
    // erstes Bild abwarten (eine frisch gestartete virtuelle Oberfläche braucht einen Moment)
    let t = std::time::Instant::now();
    let mut letzt = Err(String::from("kein Bild"));
    while t.elapsed() < Duration::from_secs(25) {
        letzt = runde(Vec::new(), true, 500);
        if letzt.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    letzt?;
    if !zeilen.is_empty() {
        runde(zeilen, false, 100)?;
        std::thread::sleep(Duration::from_millis(400));
    }
    let (w, h, p) = runde(Vec::new(), true, 600)?;
    let mut rgba = vec![0u8; w * h * 4];
    bildschirm::dekodieren(&p, w, h, &mut rgba)?;
    Ok((w, h, rgba))
}

fn foto(dir: &std::path::Path, ziel: &str, datei: &str) -> R {
    let (w, h, rgba) = bild_holen(dir, ziel, Vec::new())?;
    std::fs::write(datei, bildschirm::bmp(w, h, &rgba)).map_err(|e| e.to_string())?;
    println!("{datei}: {w}×{h}");
    Ok(())
}

fn fern(dir: &std::path::Path, ziel: &str, zeilen: &[&str]) -> R {
    let (w, h, _) = bild_holen(dir, ziel, zeilen.iter().map(|s| s.to_string()).collect())?;
    println!("gesendet ({} Eingaben), Bildschirm {w}×{h}", zeilen.len());
    Ok(())
}

fn senden(dir: &std::path::Path, ziel: &str, datei: &str, oeffnen: bool) -> R {
    let daten = std::fs::read(datei).map_err(|e| format!("{datei}: {e}"))?;
    if daten.len() > weitergeben::MAX_DATEI {
        return Err("Datei zu groß (höchstens 48 MB)".into());
    }
    let name = std::path::Path::new(datei).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "datei".into());
    antwort_zeigen(frage(dir, ziel, Nachricht::Datei { name, daten: base64::engine::general_purpose::STANDARD.encode(&daten), oeffnen })?)
}

fn hoeren(dir: &std::path::Path, ziel: &str, sek: u64) -> R {
    let (mut k, g, ..) = kanal_zu(dir, ziel)?;
    k.zeitlimit(Some(Duration::from_secs(10)));
    k.nachricht_senden(&Nachricht::TonHoeren).map_err(|e| e.to_string())?;
    if let Nachricht::Fehler { text } = k.nachricht_empfangen().map_err(|e| e.to_string())? {
        return Err(text);
    }
    eprintln!("[verbund] höre „{}“ … (Strg+C beendet)", g.name);
    let mut w = ton::Wiedergabe::start()?;
    let t = std::time::Instant::now();
    let mut bytes = 0usize;
    while sek == 0 || t.elapsed() < Duration::from_secs(sek) {
        k.nachricht_senden(&Nachricht::TonWeiter).map_err(|e| e.to_string())?;
        match k.nachricht_empfangen().map_err(|e| e.to_string())? {
            Nachricht::TonDaten { pcm } => {
                let roh = base64::engine::general_purpose::STANDARD.decode(pcm).unwrap_or_default();
                bytes += roh.len();
                w.spielen(&roh)?;
            }
            Nachricht::Fehler { text } => return Err(text),
            _ => return Err("unerwartet".into()),
        }
    }
    eprintln!("[verbund] {:.1} s Ton empfangen", bytes as f64 / (ton::RATE as f64 * 2.0));
    Ok(())
}

fn sprechen(dir: &std::path::Path, ziel: &str, sek: u64) -> R {
    let (mut k, g, ..) = kanal_zu(dir, ziel)?;
    k.zeitlimit(Some(Duration::from_secs(10)));
    let a = ton::Aufnahme::start()?;
    eprintln!("[verbund] Durchsage auf „{}“ … (Strg+C beendet)", g.name);
    let t = std::time::Instant::now();
    let mut bytes = 0usize;
    while sek == 0 || t.elapsed() < Duration::from_secs(sek) {
        let pcm = a.holen(200);
        if pcm.is_empty() {
            if a.rx.try_recv().is_err() && t.elapsed() > Duration::from_secs(2) && bytes == 0 {
                return Err("keine Aufnahme".into());
            }
            continue;
        }
        bytes += pcm.len();
        k.nachricht_senden(&Nachricht::TonDaten { pcm: base64::engine::general_purpose::STANDARD.encode(&pcm) }).map_err(|e| e.to_string())?;
        if let Nachricht::Fehler { text } = k.nachricht_empfangen().map_err(|e| e.to_string())? {
            return Err(text);
        }
    }
    eprintln!("[verbund] {:.1} s gesprochen", bytes as f64 / (ton::RATE as f64 * 2.0));
    Ok(())
}

fn mining(dir: &std::path::Path, rest: &[&str]) -> R {
    use mining::Konfig;
    let mut k = Konfig::laden(dir);
    match rest {
        [] | ["status"] => {
            let s = mining::status(dir);
            println!("Mining      {} ({})", if s.aktiv { "AN" } else { "AUS" }, if s.laeuft { format!("läuft seit {} min", s.minuten) } else { s.grund.clone() });
            println!("Währung     {}  Pool {}  Wallet {}", k.waehrung, k.pool, if k.wallet.is_empty() { "–".into() } else { format!("{}…", &k.wallet[..k.wallet.len().min(12)]) });
            println!("Regeln      nur im Leerlauf ({} min), {} % der Kerne, nur am Strom: {}, bis {} °C", k.leerlauf_min, k.kerne_prozent, k.nur_am_strom, k.max_temp_c);
            println!("Miner       {}", if s.installiert { mining::miner_pfad(dir).display().to_string() } else { "nicht eingerichtet (ultar-verbund mining einrichten)".into() });
            if s.laeuft {
                println!("Leistung    {:.0} H/s", s.hashrate);
            }
            return Ok(());
        }
        ["an"] => {
            k.aktiv = true;
            k.pruefen()?;
        }
        ["aus"] => {
            k.aktiv = false;
            mining::MINER.lock().unwrap().stoppen();
        }
        ["einrichten"] => {
            println!("{}", mining::installieren(dir)?);
            return Ok(());
        }
        ["wallet", w] => k.wallet = w.trim().to_string(),
        ["pool", p] => k.pool = p.to_string(),
        ["kerne", p] => k.kerne_prozent = p.parse().map_err(|_| "Prozent 1..100")?,
        ["leerlauf", m] => k.leerlauf_min = m.parse().map_err(|_| "Minuten")?,
        ["waehrung", w] => {
            k.waehrung = w.to_string();
            if *w != "eigen" {
                k.pruefen().map(|_| ()).or_else(|e| if e.contains("Wallet") { Ok(()) } else { Err(e) })?;
            }
        }
        ["alle", rest @ ..] => {
            k.pruefen()?;
            let ich = Ident::laden(dir).map_err(|e| e.to_string())?;
            let c = Config::laden(dir);
            let mut ziel_k = k.clone();
            ziel_k.arbeiter = String::new(); // jedes Gerät meldet sich mit seinem eigenen Namen
            let konfig = serde_json::to_value(&ziel_k).unwrap();
            let einrichten = !rest.contains(&"--ohne-einrichten");
            let mut ok = 0;
            for g in geraete_finden(dir, &c, &ich).into_iter().filter(|g| c.partner(&g.pk).is_some()) {
                match frage(dir, &g.kennung, Nachricht::Mining { konfig: Some(konfig.clone()), einrichten }) {
                    Ok(Nachricht::MiningStatus { .. }) => {
                        ok += 1;
                        println!("{}  „{}“ eingerichtet", g.kennung, g.name);
                    }
                    Ok(Nachricht::Fehler { text }) | Err(text) => println!("{}  „{}“: {text}", g.kennung, g.name),
                    Ok(_) => {}
                }
            }
            k.speichern(dir).map_err(|e| e.to_string())?;
            if einrichten && !mining::installiert(dir) {
                match mining::installieren(dir) {
                    Ok(t) => println!("hier: {t}"),
                    Err(e) => println!("hier: {e}"),
                }
            }
            println!("Mining auf {ok} weiteren Geräten eingestellt – läuft nur, wenn ein Gerät ruht");
            return Ok(());
        }
        _ => return Err("mining status|an|aus|einrichten|wallet ADR|pool HOST:PORT|kerne PROZENT|leerlauf MIN|waehrung xmr|eigen|alle".into()),
    }
    k.speichern(dir).map_err(|e| e.to_string())?;
    println!("gespeichert");
    Ok(())
}
