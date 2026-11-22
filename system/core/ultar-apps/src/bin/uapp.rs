//! uapp – Apps aller Formate installieren, starten, entfernen (Ultar OS).
//!
//!   uapp detect  <datei>                           Format erkennen
//!   uapp install <datei> [--silent] [--allow-foreign] [--name NAME]
//!   uapp list [--json]
//!   uapp run <id>
//!   uapp remove <id>
//!   uapp android-sync                              ausstehende APKs in die laufende Android-Umgebung
//!   uapp policy                                    aktuelle Richtlinie anzeigen

use std::path::Path;
use ultar_apps as ua;

fn usage() -> ! {
    eprintln!("{}", ultar_sprache::tr("Benutzung: uapp detect|install|list|run|remove|android-sync|policy …\n  uapp install <datei> [--silent] [--allow-foreign] [--name NAME]"));
    std::process::exit(2)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let p = ua::Paths::system();
    let mut log = |l: &str| println!("{l}");
    let r: Result<(), String> = match a.first().map(String::as_str) {
        Some("detect") => {
            let f = a.get(1).unwrap_or_else(|| usage());
            ua::detect(Path::new(f))
                .map(|fm| println!("{} ({:?})", fm.label(), fm))
                .map_err(|e| e.to_string())
        }
        Some("install") => {
            let mut o = ua::Opts::default();
            let mut file = None;
            let mut i = 1;
            while i < a.len() {
                match a[i].as_str() {
                    "--silent" => o.silent = true,
                    "--allow-foreign" => o.allow_foreign = true,
                    "--name" => {
                        i += 1;
                        o.name = a.get(i).cloned();
                    }
                    f => file = Some(f.to_string()),
                }
                i += 1;
            }
            let f = file.unwrap_or_else(|| usage());
            ua::install(&p, Path::new(&f), &o, &mut log).map(|e| {
                if e.status == "pending" {
                    println!("STATUS pending {}", e.id);
                } else {
                    println!("STATUS ok {}", e.id);
                }
            })
        }
        Some("list") => {
            let l = ua::list(&p);
            if a.get(1).is_some_and(|x| x == "--json") {
                // einfache, abhängigkeitsfreie Ausgabe für die Web-Verwaltung
                let items: Vec<String> = l.iter().map(|e| format!("{{\"id\":{:?},\"name\":{:?},\"format\":{:?},\"status\":{:?},\"note\":{:?}}}", e.id, e.name, e.format.map(|f| f.label()).unwrap_or(""), e.status, e.note)).collect();
                println!("[{}]", items.join(","));
            } else if l.is_empty() {
                println!("{}", ultar_sprache::tr("Keine Apps installiert."));
            } else {
                for e in l {
                    println!(
                        "{:28} {:34} {:8} {}",
                        e.id,
                        e.name,
                        e.status,
                        e.format.map(|f| f.label()).unwrap_or("")
                    );
                }
            }
            Ok(())
        }
        Some("run") => {
            let id = a.get(1).unwrap_or_else(|| usage());
            ua::run(&p, id).and_then(|mut c| c.wait().map(|_| ()).map_err(|e| e.to_string()))
        }
        Some("remove") | Some("uninstall") => {
            let id = a.get(1).unwrap_or_else(|| usage());
            ua::remove(&p, id, &mut log)
        }
        Some("android-sync") => ua::android_sync(&p, &mut log)
            .map(|n| println!("{}", ultar_sprache::tr(&format!("{n} App(s) installiert")))),
        Some("policy") => {
            let pol = ua::Policy::load(&p);
            println!("Ausgabe: {}\nfremde Formate erlaubt: {}\nWindows-Apps abgeschottet: {}\ngesperrt: {:?}", ultar_hal_system::current_edition(), pol.allow_foreign, pol.wine_isolate, pol.deny_formats);
            Ok(())
        }
        _ => usage(),
    };
    if let Err(e) = r {
        eprintln!("uapp: {}", ultar_sprache::tr(&e.to_string()));
        std::process::exit(1);
    }
}
