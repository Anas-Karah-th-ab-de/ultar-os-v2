//! `ultar-calld` – the call service daemon.
//!
//! `ultar-calld --bus SOCKET [--log FILE] [--modem] [--fake-modem] [--ultar OWN_NUMBER] [--lang ar|en|de]`
//! * `--modem`: the mobile network through the modem service (`modem.*`, ultar-ril);
//! * `--fake-modem`: the test double of the modem in this process (tests on the Ultar kernel);
//! * `--ultar`: the Ultar-number way over the device backend, with the own number.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::call::{Cellular, Output};
use ultar_bus::{Client, ClientOptions};
use ultar_call::fake::FakeAudio;
use ultar_call::fake_modem::{FakeModem, FakeModemControl};
use ultar_call::line::SystemClock;
use ultar_call::modem_line::{ModemAudio, ModemLine};
use ultar_call::service::{Service, DEFAULT_TICK};
use ultar_call::session::Session;
use ultar_call::ultar_line::UltarLine;
use ultar_call::Line;

/// Time a modem command may take.
const MODEM_WAIT: Duration = Duration::from_secs(5);

use ultar_call::msg::{text, Msg};

/// Command line.
#[derive(Default)]
struct Args {
    bus: Option<PathBuf>,
    log: Option<PathBuf>,
    modem: bool,
    fake_modem: bool,
    ultar: Option<String>,
    lang: String,
}

fn parse(list: &[String]) -> Option<Args> {
    let mut a = Args {
        lang: "ar".into(),
        ..Args::default()
    };
    let mut it = list.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bus" => a.bus = Some(PathBuf::from(it.next()?)),
            "--log" => a.log = Some(PathBuf::from(it.next()?)),
            "--modem" => a.modem = true,
            "--fake-modem" => a.fake_modem = true,
            "--ultar" => a.ultar = Some(it.next()?.clone()),
            "--lang" => a.lang = it.next()?.clone(),
            _ => return None,
        }
    }
    a.bus.as_ref()?;
    Some(a)
}

fn connect(path: &std::path::Path, name: &str) -> Option<Client> {
    Client::connect(path, ClientOptions::named(name)).ok()
}

/// Starts the fake modem in a thread of this process.
fn start_fake_modem(path: &std::path::Path) -> Option<()> {
    let control = FakeModemControl::default();
    control.set_cellular(Cellular::Ready, &[1]);
    let mut modem = FakeModem::new(connect(path, "ril")?, control).ok()?;
    std::thread::spawn(move || while modem.step(DEFAULT_TICK).is_ok() {});
    Some(())
}

fn run(a: &Args) -> Result<(), Msg> {
    let path = a.bus.clone().ok_or(Msg::Usage)?;
    let bus = connect(&path, "calld").ok_or(Msg::NoBus)?;
    if a.fake_modem {
        start_fake_modem(&path).ok_or(Msg::NoBus)?;
    }
    let mut lines: Vec<Box<dyn Line>> = Vec::new();
    let modem = a.modem || a.fake_modem;
    if modem {
        lines.push(Box::new(
            ModemLine::new(bus.clone(), 1, MODEM_WAIT).map_err(|_| Msg::NoBus)?,
        ));
    }
    if let Some(own) = &a.ultar {
        lines.push(Box::new(UltarLine::new(
            ultar_call::default_backend(),
            Some(own.clone()),
        )));
    }
    let audio: Box<dyn ultar_call::line::AudioRoute> = if modem {
        Box::new(ModemAudio::new(bus.clone(), MODEM_WAIT))
    } else {
        Box::new(FakeAudio::new(&[Output::Earpiece]))
    };
    let mut session = Session::new(lines, audio, Box::new(SystemClock::default()));
    if let Some(log) = &a.log {
        if session.with_log_file(log) > 0 {
            eprintln!("{}", text(Msg::BadLog, &a.lang));
        }
    }
    let service = Service::new(bus, session, DEFAULT_TICK).map_err(|_| Msg::NoBus)?;
    service.run().map_err(|_| Msg::Stopped)
}

fn main() {
    let list: Vec<String> = std::env::args().skip(1).collect();
    let Some(args) = parse(&list) else {
        eprintln!("{}", text(Msg::Usage, "ar"));
        std::process::exit(2);
    };
    if let Err(msg) = run(&args) {
        eprintln!("{}", text(msg, &args.lang));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn arguments() {
        assert!(parse(&v(&[])).is_none(), "--bus is required");
        assert!(parse(&v(&["--bus"])).is_none());
        assert!(parse(&v(&["--bus", "/b", "--what"])).is_none());
        let a = parse(&v(&[
            "--bus", "/b", "--log", "/l", "--modem", "--ultar", "UOS-1", "--lang", "en",
        ]))
        .unwrap();
        assert!(a.modem && !a.fake_modem);
        assert_eq!(a.ultar.as_deref(), Some("UOS-1"));
        assert_eq!(text(Msg::NoBus, &a.lang), "ultar-calld: bus not reachable");
        assert!(text(Msg::Usage, "ar").starts_with("الاستخدام"));
    }
}
