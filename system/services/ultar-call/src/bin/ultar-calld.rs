//! `ultar-calld` – the call service daemon.
//!
//! `ultar-calld --bus SOCKET [--log FILE] [--modem] [--fake-modem] [--ultar OWN_NUMBER]
//!              [--account] [--pcm NODE] [--camera NODE] [--lang ar|en|de]
//!              [--mesh-calls] [--mesh-guest] [--mesh-modem-pcm NODE]`
//! * `--modem`: the mobile network through the modem service (`modem.*`, ultar-ril);
//! * `--fake-modem`: the test double of the modem in this process (tests on the Ultar kernel);
//! * `--ultar`: the Ultar-number way over the local mock backend, with this own number (tests);
//! * `--account`: the Ultar-number way end to end with the person's account (`account.*`):
//!   signalling at the account server, the device key in the account service, sound through
//!   `--pcm` (default `/dev/ultar/pcm`), pictures from `--camera` (default `/dev/ultar/kamera`);
//!   the pinned keys of other numbers lie beside `--log`;
//! * `--mesh-calls`: a ringing call is offered to the person's paired devices that may carry it
//!   (the first that answers takes it; the voice goes over the end-to-end encrypted mesh);
//!   Ultar-number calls need `--account`, mobile calls need `--mesh-modem-pcm` (the modem's
//!   voice stream), else they are not offered;
//! * `--mesh-guest`: this device answers calls of another device: the sound card (`--pcm`)
//!   is opened only while such a call runs.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::messages::call::{Cellular, Output};
use ultar_bus::{Client, ClientOptions};
use ultar_call::busdev::BusDevice;
use ultar_call::e2e::backend::MuteRoute;
use ultar_call::fake::FakeAudio;
use ultar_call::fake_modem::{FakeModem, FakeModemControl};
use ultar_call::line::SystemClock;
use ultar_call::mesh_relay::{MeshRelay, Taps};
use ultar_call::mesh_tap::{Bridge, CardTap, MeshTap};
use ultar_call::modem_line::{ModemAudio, ModemLine};
use ultar_call::service::{Service, DEFAULT_TICK};
use ultar_call::session::Session;
use ultar_call::ultar_line::UltarLine;
use ultar_call::{E2eCall, Line};

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
    account: bool,
    pcm: Option<PathBuf>,
    camera: Option<PathBuf>,
    mesh_calls: bool,
    mesh_guest: bool,
    mesh_modem_pcm: Option<PathBuf>,
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
            "--account" => a.account = true,
            "--pcm" => a.pcm = Some(PathBuf::from(it.next()?)),
            "--camera" => a.camera = Some(PathBuf::from(it.next()?)),
            "--mesh-calls" => a.mesh_calls = true,
            "--mesh-guest" => a.mesh_guest = true,
            "--mesh-modem-pcm" => a.mesh_modem_pcm = Some(PathBuf::from(it.next()?)),
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

/// What the calls of this device do over the mesh (`--mesh-calls`: it keeps the line and
/// offers calls; `--mesh-guest`: it answers calls of another device on its sound card).
fn mesh_relay(a: &Args, path: &Path, bridge: &Bridge) -> Result<MeshRelay, Msg> {
    let mut taps = Taps::default();
    if a.mesh_calls {
        if a.account {
            taps.ultar = Some(Box::new(bridge.tap()));
        }
        taps.cellular = a
            .mesh_modem_pcm
            .clone()
            .map(|node| Box::new(CardTap::lazy(node)) as Box<dyn MeshTap>);
    }
    if a.mesh_guest {
        let node = a
            .pcm
            .clone()
            .unwrap_or_else(|| ultar_hal_system::seat_nodes::pcm_node(Path::new("/")));
        taps.guest = Some(Box::new(CardTap::lazy(node)));
    }
    let mesh = connect(path, "calld").ok_or(Msg::NoBus)?;
    Ok(MeshRelay::new(mesh, taps))
}
/// The microphone switch and the outputs of the lines this device has.
fn audio_route(
    bus: &Client,
    modem: bool,
    muted: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Box<dyn ultar_call::line::AudioRoute> {
    match (modem, muted) {
        (true, Some(flag)) => Box::new(BothRoute {
            modem: ModemAudio::new(bus.clone(), MODEM_WAIT),
            ultar: MuteRoute::new(flag),
        }),
        (true, None) => Box::new(ModemAudio::new(bus.clone(), MODEM_WAIT)),
        (false, Some(flag)) => Box::new(MuteRoute::new(flag)),
        (false, None) => Box::new(FakeAudio::new(&[Output::Earpiece])),
    }
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
    let mut muted = None;
    let bridge = Bridge::default();
    if a.account {
        let dir = a
            .log
            .as_ref()
            .and_then(|l| l.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/data/ultar/calls"));
        let pcm = a
            .pcm
            .clone()
            .unwrap_or_else(|| ultar_hal_system::seat_nodes::pcm_node(Path::new("/")));
        let account = connect(&path, "calld").ok_or(Msg::NoBus)?;
        let camera = a
            .camera
            .clone()
            .unwrap_or_else(|| ultar_hal_system::seat_nodes::camera_node(Path::new("/")));
        let device = BusDevice::start_with(account, dir, pcm, camera, bridge.clone());
        let backend = E2eCall::new(device);
        muted = Some(backend.muted.clone());
        lines.push(Box::new(UltarLine::new(Box::new(backend), None)));
    }
    let audio = audio_route(&bus, modem, muted);
    let mut session = Session::new(lines, audio, Box::new(SystemClock::default()));
    if let Some(log) = &a.log {
        if session.with_log_file(log) > 0 {
            eprintln!("{}", text(Msg::BadLog, &a.lang));
        }
    }
    let mut service = Service::new(bus, session, DEFAULT_TICK).map_err(|_| Msg::NoBus)?;
    if a.mesh_calls || a.mesh_guest {
        service = service.with_mesh_relay(mesh_relay(a, &path, &bridge)?);
    }
    service.run().map_err(|_| Msg::Stopped)
}

/// Microphone switch for both ways on a phone: the modem's and the Ultar calls'; the outputs are
/// the modem's.
struct BothRoute {
    modem: ModemAudio,
    ultar: MuteRoute,
}

impl ultar_call::line::AudioRoute for BothRoute {
    fn set_mute(&mut self, on: bool) -> Result<(), ultar_call::line::LineError> {
        self.ultar.set_mute(on)?;
        self.modem.set_mute(on)
    }
    fn set_output(&mut self, output: Output) -> Result<(), ultar_call::line::LineError> {
        self.modem.set_output(output)
    }
    fn outputs(&self) -> Vec<Output> {
        self.modem.outputs()
    }
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
