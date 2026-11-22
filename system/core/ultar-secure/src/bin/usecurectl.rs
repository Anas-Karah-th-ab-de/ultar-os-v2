//! Steuerwerkzeug für Geräteverschlüsselung (über die Live-Tür oder die Shell aufrufbar).
//! Aufrufe: status | isenc | isopen | unlock <pw> | enable <pw> | passwd <alt> <neu>
//!          | disable <pw> | close | veritystatus
use ultar_secure::{udata, verity};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cmd = a.get(1).map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => {
            println!("encrypted={}", udata::is_encrypted());
            println!("open={}", udata::is_open());
            if let Some(s) = verity::status("ultar-root") {
                println!("verity=aktiv");
                println!("{s}");
            } else {
                println!("verity=inaktiv");
            }
        }
        "isenc" => println!("{}", udata::is_encrypted()),
        "isopen" => println!("{}", udata::is_open()),
        "unlock" => report(udata::unlock(arg(&a, 2))),
        "enable" => report(udata::enable(arg(&a, 2))),
        "passwd" => report(udata::change_password(arg(&a, 2), arg(&a, 3))),
        "disable" => report(udata::disable(arg(&a, 2))),
        "close" => report(udata::close()),
        "veritystatus" => match verity::status("ultar-root") {
            Some(s) => println!("{s}"),
            None => println!("inaktiv"),
        },
        _ => {
            eprintln!("unbekannt: {cmd}");
            std::process::exit(2);
        }
    }
}

fn arg<'a>(a: &'a [String], i: usize) -> &'a str {
    a.get(i).map(|s| s.as_str()).unwrap_or("")
}

fn report(r: Result<(), String>) {
    match r {
        Ok(()) => println!("OK"),
        Err(e) => {
            eprintln!("FEHLER: {e}");
            std::process::exit(1);
        }
    }
}
