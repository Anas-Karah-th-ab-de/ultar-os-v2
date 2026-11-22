//! unotify — Benachrichtigung aus Dienst/Skript/Tür senden.
//! Aufruf: unotify <kind> <app> <titel> [text] [--urgency low|normal|high]
//! kind: call | sms | message | system | app
use ultar_notify::{post, Urgency};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut urg = Urgency::Normal;
    if let Some(p) = args.iter().position(|a| a == "--urgency") {
        if let Some(v) = args.get(p + 1).cloned() {
            urg = Urgency::parse(&v);
        }
        args.drain(p..=(p + 1).min(args.len() - 1));
    }
    if args.len() < 3 {
        eprintln!("Aufruf: unotify <kind> <app> <titel> [text] [--urgency low|normal|high]");
        std::process::exit(2);
    }
    let kind = &args[0];
    let app = &args[1];
    let title = &args[2];
    let body = args.get(3).map(|s| s.as_str()).unwrap_or("");
    post(kind, app, title, body, urg);
}
