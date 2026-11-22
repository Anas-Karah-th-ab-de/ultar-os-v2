//! zelle – Apps und Container abgeschottet starten, verknüpfen, Grafikkarten verteilen, Netz protokollieren.

use std::time::Duration;
use ultar_sprache::{eprintln, println};
use ultar_zellen::profil::{gueltiger_name, Profil};
use ultar_zellen::*;

const HILFE: &str = ultar_sprache::tabellen::ZELLE_HILFE_DE;

type R = Result<(), String>;

fn main() {
    let a: Vec<String> = ultar_sprache::argumente();
    let s: Vec<&str> = a.iter().map(|x| x.as_str()).collect();
    let r: R = match s.as_slice() {
        ["innen", name] => std::process::exit(zelle::innen(name)),
        ["waechter", name] => waechter::laufen(name),
        ["neu", name, rest @ ..] => neu(name, rest),
        ["liste"] => liste(false),
        ["liste", "--json"] => liste(true),
        ["zeige", name] => {
            Profil::laden(name).map(|p| println!("{}", toml::to_string_pretty(&p).unwrap()))
        }
        ["loeschen", name] => loeschen(name),
        ["setze", name, feld, wert] => setze(name, feld, wert),
        ["start", name, rest @ ..] => start(name, rest),
        ["stopp", name] => {
            zelle::stoppen(name);
            println!("{name} gestoppt");
            Ok(())
        }
        ["verknuepfen", x, y] => verknuepfen(x, y, true),
        ["trennen", x, y] => verknuepfen(x, y, false),
        ["verknuepfen-container", x, y] => kiri::Docker::standard()
            .verknuepfen(x, y)
            .map(|t| println!("{t}")),
        ["protokoll", name, rest @ ..] => protokoll(name, rest),
        ["systemprotokoll", rest @ ..] => {
            let runden = rest
                .iter()
                .position(|x| *x == "--runden")
                .and_then(|i| rest.get(i + 1))
                .and_then(|x| x.parse().ok());
            waechter::system(Duration::from_secs(2), runden)
        }
        ["empfehlung", neu, rest @ ..] => empfehlung(neu, rest.contains(&"--json")),
        ["gpu"] => gpu_zeigen(false),
        ["gpu", "--json"] => gpu_zeigen(true),
        ["gpu", "modus", m] => {
            std::fs::create_dir_all(wurzel()).ok();
            std::fs::write(wurzel().join("gpu.toml"), format!("modus = \"{m}\"\n"))
                .map_err(|e| e.to_string())
                .map(|_| println!("Grafik-Vorgabe: {m}"))
        }
        ["netz"] => netz_zeigen(false),
        ["netz", "--json"] => netz_zeigen(true),
        ["netz", "geraet", w] => {
            netz_setzen(|r| r.geraet = matches!(*w, "an" | "on" | "true" | "ja"))
        }
        ["netz", "vorgabe", w] if matches!(*w, "frei" | "aus") => {
            netz_setzen(|r| r.vorgabe = w.to_string())
        }
        ["netz", "erlauben", d] => netz_dienst(d, true),
        ["netz", "verbieten", d] => netz_dienst(d, false),
        ["netz", "anwenden"] => ultar_zellen::regeln::Regeln::laden().anwenden(),
        ["kiri", "status"] => {
            let d = kiri::Docker::standard();
            println!(
                "Kiri eingerichtet: {}",
                if kiri::installiert() {
                    "ja"
                } else {
                    "nein (zelle kiri einrichten)"
                }
            );
            match d.version() {
                Ok(v) => println!("Dienst läuft: {v} ({})", d.socket.display()),
                Err(e) => println!("Dienst: {e}"),
            }
            Ok(())
        }
        ["kiri", "status", "--json"] => {
            let d = kiri::Docker::standard();
            let n = d.container().map(|c| c.len()).unwrap_or(0);
            println!(
                "{}",
                serde_json::json!({"installiert": kiri::installiert(), "dienst": d.version().ok(), "container": n})
            );
            Ok(())
        }
        ["kiri", "einrichten"] => kiri::einrichten().map(|t| println!("{t}")),
        ["kiri", "container"] => kiri::Docker::standard().container().map(|cs| {
            for c in cs {
                println!(
                    "{:<14} {:<24} {:<30} {}",
                    &c.id[..c.id.len().min(12)],
                    c.name,
                    c.abbild,
                    c.zustand
                );
            }
        }),
        _ => {
            println!("{HILFE}");
            Ok(())
        }
    };
    if let Err(e) = r {
        eprintln!("zelle: {e}");
        std::process::exit(1);
    }
}

fn wert<'a>(rest: &'a [&str], k: &str) -> Option<&'a str> {
    rest.iter()
        .position(|x| *x == k)
        .and_then(|i| rest.get(i + 1))
        .copied()
}

fn neu(name: &str, rest: &[&str]) -> R {
    if !gueltiger_name(name) {
        return Err("Name: nur a-z, 0-9 und -, höchstens 32 Zeichen".into());
    }
    if profil::ordner(name).join("zelle.toml").exists() {
        return Err(format!("„{name}“ gibt es schon"));
    }
    let mut p = Profil {
        name: name.into(),
        ..Default::default()
    };
    // Netz neuer Zellen nach der Vorgabe des Geräts (Einstellungen → Internet-Regeln)
    p.netz.art = ultar_zellen::regeln::Regeln::laden().vorgabe;
    let (opts, befehl) = match rest.iter().position(|x| *x == "--") {
        Some(i) => (
            &rest[..i],
            rest[i + 1..].iter().map(|s| s.to_string()).collect(),
        ),
        None => (rest, Vec::new()),
    };
    p.befehl = befehl;
    if let Some(app) = wert(opts, "--app") {
        p.app = app.into();
    }
    if let Some(n) = wert(opts, "--netz") {
        match n.split_once(':') {
            Some(("ueber", v)) => {
                p.netz.art = "ueber".into();
                p.netz.ueber = v.into();
            }
            _ => p.netz.art = n.into(),
        }
    }
    if let Some(o) = wert(opts, "--ort") {
        let ok = o
            .split_once(',')
            .map(|(a, b)| {
                a.trim().parse::<f64>().is_ok_and(|x| x.abs() <= 90.0)
                    && b.trim().parse::<f64>().is_ok_and(|x| x.abs() <= 180.0)
            })
            .unwrap_or(false);
        if !ok {
            return Err("--ort BREITE,LAENGE (z. B. 52.52,13.40)".into());
        }
        p.privat.ort = o.replace(' ', "");
    }
    if let Some(z) = wert(opts, "--zeitzone") {
        p.privat.zeitzone = z.into();
    }
    if let Some(l) = wert(opts, "--sprache") {
        p.privat.sprache = l.into();
    }
    if let Some(i) = wert(opts, "--vpn-zelle") {
        p.netz.vpn_zelle = true;
        p.netz.vpn_schnittstelle = i.into();
    }
    for (k, f) in [("--wg-conf", 0), ("--wg-adresse", 1), ("--wg-endpunkt", 2)] {
        if let Some(v) = wert(opts, k) {
            *[
                &mut p.netz.wg_conf,
                &mut p.netz.wg_adresse,
                &mut p.netz.wg_endpunkt,
            ][f] = v.into();
        }
    }
    p.protokoll.verbindungen = opts.contains(&"--protokoll") || opts.contains(&"--uploads");
    p.protokoll.uploads = opts.contains(&"--uploads");
    if let Some(g) = wert(opts, "--gpu") {
        p.gpu = g.into();
    }
    if let Some(r) = wert(opts, "--ram") {
        p.speicher.ram_mb = r.parse().map_err(|_| "--ram MB")?;
    }
    if let Some(r) = wert(opts, "--ram-garantiert") {
        p.speicher.ram_garantiert_mb = r.parse().map_err(|_| "--ram-garantiert MB")?;
    }
    if let Some(r) = wert(opts, "--max-gb") {
        p.speicher.max_gb = r.parse().map_err(|_| "--max-gb N")?;
    }
    if let Some(w) = wert(opts, "--wurzel") {
        p.wurzel = w.into();
    }
    p.privat.standort = opts.contains(&"--standort");
    p.privat.geraeteinfo = opts.contains(&"--geraeteinfo");
    p.privat.kamera = opts.contains(&"--kamera");
    p.privat.mikrofon = opts.contains(&"--mikrofon");
    p.privat.usb = opts.contains(&"--usb");
    if p.app.is_empty() && p.befehl.is_empty() {
        return Err("--app ID oder -- befehl angeben".into());
    }
    if !["frei", "aus", "ueber", "wireguard"].contains(&p.netz.art.as_str()) {
        return Err("--netz frei|aus|ueber:<vpn-zelle>|wireguard".into());
    }
    if p.netz.art == "ueber" {
        let v = Profil::laden(&p.netz.ueber)?;
        if !v.netz.vpn_zelle {
            return Err(format!(
                "„{}“ ist keine VPN-Zelle (--vpn-zelle beim Anlegen)",
                v.name
            ));
        }
    }
    p.speichern()?;
    println!(
        "Zelle „{name}“ angelegt: Netz {}, Protokoll {}, Standort {}, Geräteinfo {}",
        p.netz.art,
        if p.protokoll.verbindungen {
            "an"
        } else {
            "aus"
        },
        if p.privat.standort {
            "erlaubt"
        } else {
            "gesperrt"
        },
        if p.privat.geraeteinfo {
            "erlaubt"
        } else {
            "verborgen"
        }
    );
    Ok(())
}

fn liste(json: bool) -> R {
    let v: Vec<serde_json::Value> = Profil::alle()
        .iter()
        .map(|p| {
            serde_json::json!({"name": p.name, "app": if p.app.is_empty() { p.befehl.join(" ") } else { p.app.clone() }, "netz": if p.netz.art == "ueber" { format!("über {}", p.netz.ueber) } else { p.netz.art.clone() },
                "vpn_zelle": p.netz.vpn_zelle, "laeuft": zelle::laeuft(&p.name), "belegt_mb": p.belegt() >> 20, "max_gb": p.speicher.max_gb, "ram_mb": p.speicher.ram_mb,
                "protokoll": p.protokoll.verbindungen, "uploads": p.protokoll.uploads, "standort": p.privat.standort, "geraeteinfo": p.privat.geraeteinfo, "gpu": p.gpu, "verknuepft": p.verknuepft})
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string(&v).unwrap());
        return Ok(());
    }
    for z in v {
        println!(
            "{:<16} {:<24} netz {:<14} {:>6} MB{}  {}{}",
            z["name"].as_str().unwrap_or(""),
            z["app"].as_str().unwrap_or(""),
            z["netz"].as_str().unwrap_or(""),
            z["belegt_mb"],
            if z["laeuft"] == true { "  läuft" } else { "" },
            if z["protokoll"] == true {
                "protokoll "
            } else {
                ""
            },
            z["verknuepft"]
                .as_array()
                .filter(|a| !a.is_empty())
                .map(|a| format!(
                    "↔ {}",
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                ))
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn loeschen(name: &str) -> R {
    zelle::stoppen(name);
    for mut p in Profil::alle() {
        if p.verknuepft.iter().any(|v| v == name) {
            p.verknuepft.retain(|v| v != name);
            p.speichern()?;
        }
    }
    std::fs::remove_dir_all(profil::ordner(name)).map_err(|e| e.to_string())?;
    println!("{name} gelöscht (mit eigenem Speicher)");
    Ok(())
}

fn setze(name: &str, feld: &str, w: &str) -> R {
    let p = Profil::laden(name)?;
    let mut v = toml::Value::try_from(&p).map_err(|e| e.to_string())?;
    let teile: Vec<&str> = feld.split('.').collect();
    let mut z = &mut v;
    for t in &teile[..teile.len() - 1] {
        z = z.get_mut(*t).ok_or(format!("unbekanntes Feld {feld}"))?;
    }
    let letzt = teile[teile.len() - 1];
    let alt = z
        .get(letzt)
        .ok_or(format!("unbekanntes Feld {feld}"))?
        .clone();
    let neu = match alt {
        toml::Value::Boolean(_) => toml::Value::Boolean(matches!(w, "true" | "an" | "ja" | "1")),
        toml::Value::Integer(_) => toml::Value::Integer(w.parse().map_err(|_| "Zahl erwartet")?),
        toml::Value::Array(_) => toml::Value::Array(
            w.split(',')
                .filter(|x| !x.is_empty())
                .map(|x| toml::Value::String(x.into()))
                .collect(),
        ),
        _ => toml::Value::String(w.into()),
    };
    z.as_table_mut().ok_or("Feld")?.insert(letzt.into(), neu);
    let p2: Profil = v.try_into().map_err(|e: toml::de::Error| e.to_string())?;
    p2.speichern()?;
    println!("{name}: {feld} = {w}");
    Ok(())
}

fn start(name: &str, rest: &[&str]) -> R {
    let p = Profil::laden(name)?;
    if p.speicher.max_gb > 0 && p.belegt() > p.speicher.max_gb << 30 {
        return Err(format!("Speicher der Zelle voll ({} GB) – Grenze erhöhen: zelle setze {name} speicher.max_gb …", p.speicher.max_gb));
    }
    let extra: Vec<String> = rest
        .iter()
        .skip_while(|x| **x == "--")
        .map(|s| s.to_string())
        .collect();
    let (mut k, b) = zelle::starten(&p, &extra)?;
    eprintln!(
        "[zelle] {} läuft: Netz {}{}, Grafik: {}",
        b.name,
        b.netz,
        if b.ip.is_empty() {
            String::new()
        } else {
            format!(" ({})", b.ip)
        },
        if b.gpu.is_empty() {
            "–".into()
        } else {
            b.gpu
        }
    );
    let st = k.wait().map_err(|e| e.to_string())?;
    // Netz bleibt nur für laufende Zellen
    if !p.netz.vpn_zelle {
        ultar_zellen::netz::abbauen(name);
    }
    std::process::exit(st.code().unwrap_or(1));
}

fn verknuepfen(a: &str, b: &str, an: bool) -> R {
    let mut pa = Profil::laden(a)?;
    let mut pb = Profil::laden(b)?;
    for (p, anderer) in [(&mut pa, b), (&mut pb, a)] {
        p.verknuepft.retain(|v| v != anderer);
        if an {
            p.verknuepft.push(anderer.into());
        }
        p.speichern()?;
    }
    let _ = ultar_zellen::netz::regeln();
    if an {
        let g = zelle::gemeinsam(a, b);
        std::fs::create_dir_all(&g).map_err(|e| e.to_string())?;
        println!("{a} ↔ {b}: gemeinsamer Ordner /data/gemeinsam/<andere> ({}), erreichbar als „{b}“ bzw. „{a}“", g.display());
    } else {
        println!(
            "{a} und {b} getrennt (gemeinsamer Ordner bleibt erhalten: {})",
            zelle::gemeinsam(a, b).display()
        );
    }
    Ok(())
}

fn protokoll(name: &str, rest: &[&str]) -> R {
    let n: usize = wert(rest, "--n").and_then(|x| x.parse().ok()).unwrap_or(50);
    let pfad = if name == "system" {
        wurzel().join("system-protokoll.jsonl")
    } else {
        profil::ordner(name).join("protokoll.jsonl")
    };
    let s = std::fs::read_to_string(&pfad).map_err(|_| {
        format!("kein Protokoll für {name} (zelle setze {name} protokoll.verbindungen true)")
    })?;
    let zeilen: Vec<&str> = s.lines().collect();
    let start = zeilen.len().saturating_sub(n);
    let mut hoch = 0u64;
    for z in &zeilen[start..] {
        if rest.contains(&"--json") {
            println!("{z}");
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(z).unwrap_or_default();
        hoch += v["hoch"].as_u64().unwrap_or(0);
        println!(
            "{:<6} {:<22} {:<34} ↑{:>9} ↓{:>9}  {}",
            v["art"].as_str().unwrap_or(""),
            v["ziel"].as_str().unwrap_or(""),
            v["name"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(34)
                .collect::<String>(),
            v["hoch"],
            v["runter"],
            v["upload"]
                .as_str()
                .map(|u| u.lines().next().unwrap_or("").to_string())
                .unwrap_or_default()
        );
    }
    if !rest.contains(&"--json") {
        println!(
            "{} Einträge, gesendet zusammen {} Bytes",
            zeilen.len() - start,
            hoch
        );
    }
    Ok(())
}

fn empfehlung(neu: &str, json: bool) -> R {
    let b = empfehlung::bestand();
    match empfehlung::pruefen(neu, &b) {
        Some(e) if json => println!("{}", serde_json::to_string(&e).unwrap()),
        Some(e) => {
            println!("{} – {} gibt es schon:", e.neu, e.familie);
            for v in &e.vorhanden {
                println!("  {:<9} {} ({})", v.art, v.name, v.kennung);
            }
            println!("Empfehlung: {}", e.vorschlag);
            println!("Doppelt installieren bleibt möglich.");
        }
        None if json => println!("null"),
        None => println!("{neu}: nichts Gleichartiges installiert"),
    }
    Ok(())
}

fn gpu_zeigen(json: bool) -> R {
    let k = gpu::karten();
    let w = gpu::waehlen(&gpu::vorgabe(), &k);
    if json {
        println!(
            "{}",
            serde_json::json!({"karten": k, "vorgabe": gpu::vorgabe(), "wahl": w})
        );
        return Ok(());
    }
    if k.is_empty() {
        println!("keine Grafikkarte gefunden");
    }
    for c in &k {
        println!(
            "Karte {}  {:<28} {:<8} {:>6} MB, {:>6} MB frei, {:>3} % belegt  {}",
            c.nr, c.name, c.treiber, c.vram_mb, c.vram_frei_mb, c.last, c.render
        );
    }
    println!("Vorgabe: {} → {}", gpu::vorgabe(), w.begruendung);
    Ok(())
}

fn netz_zeigen(json: bool) -> R {
    let r = ultar_zellen::regeln::Regeln::laden();
    let online: Vec<String> = Profil::alle()
        .into_iter()
        .filter(|p| p.netz.art != "aus")
        .map(|p| p.name)
        .collect();
    if json {
        println!(
            "{}",
            serde_json::json!({"geraet": r.geraet, "vorgabe": r.vorgabe, "erlaubt": r.erlaubt, "zellen_online": online})
        );
        return Ok(());
    }
    println!(
        "Internet für das Gerät selbst: {}{}",
        if r.geraet { "an" } else { "aus" },
        if r.geraet {
            ""
        } else {
            " – Weiterleitung für Zellen bleibt aktiv"
        }
    );
    println!("Netz neuer Zellen: {}", r.vorgabe);
    println!(
        "Systemdienste mit Internet: {}",
        if r.erlaubt.is_empty() {
            "keine".into()
        } else {
            r.erlaubt.join(", ")
        }
    );
    println!(
        "Zellen mit Internet: {}",
        if online.is_empty() {
            "keine".into()
        } else {
            online.join(", ")
        }
    );
    Ok(())
}

fn netz_setzen(f: impl FnOnce(&mut ultar_zellen::regeln::Regeln)) -> R {
    let mut r = ultar_zellen::regeln::Regeln::laden();
    f(&mut r);
    r.speichern()?;
    r.anwenden()?;
    netz_zeigen(false)
}

fn netz_dienst(d: &str, an: bool) -> R {
    let mut r = ultar_zellen::regeln::Regeln::laden();
    r.erlaubt.retain(|x| x != d);
    if an {
        r.erlaubt.push(d.into());
    }
    r.speichern()?;
    r.anwenden()?;
    // Dienst neu starten, damit er die Gruppe bekommt bzw. verliert
    let _ = std::process::Command::new("ultar-ctl")
        .args(["restart", d])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    println!(
        "{d}{}",
        if an {
            " darf ins Internet (Neustart des Dienstes)"
        } else {
            " bleibt offline"
        }
    );
    Ok(())
}
