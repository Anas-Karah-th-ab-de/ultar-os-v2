//! ucloud — Ultar-Cloud über Tür/Shell.
//! register <user> <pass> | login <user> <pass> | number | vpn | store | update [stable|beta] | whoami
use ultar_cloud as cloud;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cmd = a.get(1).map(|s| s.as_str()).unwrap_or("whoami");
    let r: Result<(), String> = match cmd {
        "register" => cloud::register(arg(&a, 2), arg(&a, 3)).map(|_| println!("OK angemeldet")),
        "login" => cloud::login(arg(&a, 2), arg(&a, 3)).map(|_| println!("OK angemeldet")),
        "number" => cloud::register_number().map(|n| println!("Nummer: {n}")),
        "vpn" => cloud::vpn_provision().map(|_| println!("OK VPN-Peer bereitgestellt (/run/ultar/wg-peer.conf)")),
        "store" => cloud::store_list().map(|apps| {
            for app in apps {
                println!("{}  {}  v{}  {} B", app.id, app.name, app.version, app.size);
            }
        }),
        "update" => cloud::update_check(a.get(2).map(|s| s.as_str()).unwrap_or("stable")).map(|r| match r {
            Some(rel) => println!("Update: v{} ({})  {}", rel.version, rel.channel, rel.url),
            None => println!("kein Update"),
        }),
        "whoami" => {
            println!("server: {}", cloud::base_url());
            println!("angemeldet: {}", cloud::token().is_some());
            println!("nummer: {}", cloud::my_number().unwrap_or_else(|| "—".into()));
            Ok(())
        }
        _ => Err(format!("unbekannt: {cmd}")),
    };
    if let Err(e) = r {
        eprintln!("FEHLER: {e}");
        std::process::exit(1);
    }
}

fn arg<'a>(a: &'a [String], i: usize) -> &'a str {
    a.get(i).map(|s| s.as_str()).unwrap_or("")
}
