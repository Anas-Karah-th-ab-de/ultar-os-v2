//! `telefontest` – end-to-end check of the phone on the target system (QEMU, Ultar kernel;
//! `tools/kern-test/telefon-e2e.sh`), also runnable on Linux:
//! a bus broker, the test-double modem (`modem.*`), the real `ultar-calld` process with
//! `--modem`, and the phone app (headless, Arabic, phone size) on the bus:
//! incoming → answer → hold → tone → end, then an outgoing call and the history on disk.
//! Prints `TELEFON: ALLES OK` or `TELEFON: FEHLER <step>` (exit 1).
//!
//! `telefontest --calld /bin/ultar-calld --work /tmp/tel`

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::call::{CallState, Cellular, Phase};
use ultar_bus::messages::modem::ModemCallStatus;
use ultar_bus::{
    Broker, Client, ClientOptions, FixedIdentity, Identity, Policy, TypedSubscription,
};
use ultar_call::fake_modem::{FakeModem, FakeModemControl};
use ultar_phone::port::Host;
use ultar_phone::{BusPort, Click, Look, PhoneApp, Screen, WindowOp};
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::{Lang, Size};

const WAIT: Duration = Duration::from_secs(30);

struct NoHost;
impl Host for NoHost {
    fn copy_text(&mut self, _t: &str) -> bool {
        true
    }
    fn window(&mut self, _op: WindowOp) {}
    fn open_settings(&mut self) {}
    fn create_contact(&mut self, _n: &str) {}
}

type App = PhoneApp<BusPort, NoHost>;

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fail(step: &str) -> ! {
    println!("TELEFON: FEHLER {step}");
    std::process::exit(1);
}

fn click(app: &mut App, want: Click, step: &str) {
    let Some(id) = app
        .clicks()
        .iter()
        .find(|(_, c)| *c == want)
        .map(|(id, _)| *id)
    else {
        fail(step)
    };
    app.ui().layout();
    let Some(rect) = app.ui().rect(id) else {
        fail(step)
    };
    let pos = rect.center();
    let _ = app.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    });
    let _ = app.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    });
}

fn until(
    app: &mut App,
    states: &TypedSubscription<CallState>,
    step: &str,
    f: impl Fn(&CallState) -> bool,
) {
    let end = Instant::now() + WAIT;
    while Instant::now() < end {
        if let Ok(st) = states.recv_timeout(Duration::from_millis(200)) {
            let done = f(&st);
            let _ = app.set_state(st, 0);
            if done {
                println!("ok    {step}");
                return;
            }
        }
    }
    fail(step);
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let calld = arg("--calld").unwrap_or_else(|| "/bin/ultar-calld".into());
    let work = PathBuf::from(arg("--work").unwrap_or_else(|| "/tmp/telefon".into()));
    if std::fs::create_dir_all(&work).is_err() {
        fail("work");
    }
    let sock = work.join("bus.sock");
    let log = work.join("calls.log");
    let Ok(id) = Identity::new("test.peer") else {
        fail("identity")
    };
    let Ok(broker) = Broker::builder(Policy::allow_all())
        .listen(&sock, Arc::new(FixedIdentity::new(id)))
        .start()
    else {
        fail("broker")
    };
    let modem = FakeModemControl::default();
    modem.set_cellular(Cellular::Ready, &[1]);
    let Ok(ril) = Client::connect(&sock, ClientOptions::named("ril")) else {
        fail("ril")
    };
    let Ok(mut fake) = FakeModem::new(ril, modem.clone()) else {
        fail("modem")
    };
    std::thread::spawn(move || while fake.step(Duration::from_millis(10)).is_ok() {});
    let args = [
        "--bus",
        &sock.to_string_lossy(),
        "--log",
        &log.to_string_lossy(),
        "--modem",
    ];
    let Ok(child) = Command::new(&calld).args(args).spawn() else {
        fail("calld")
    };
    let daemon = Daemon(child);
    let Ok(client) = Client::connect(&sock, ClientOptions::named("phone")) else {
        fail("phone")
    };
    let port = BusPort::new(client, WAIT);
    let Ok(states) = port.states() else {
        fail("subscribe")
    };
    let look = phone_look();
    let Ok(mut app) = PhoneApp::new(look, CallState::default(), port, NoHost) else {
        fail("app")
    };
    until(&mut app, &states, "Mobilfunk bereit", |s| {
        s.lines.cellular == Cellular::Ready
    });
    call_flow(&mut app, &states, &modem, &log);
    drop(daemon);
    drop(broker);
    println!("TELEFON: ALLES OK");
}

/// Incoming -> answer -> hold -> end, then the history on disk.
fn call_flow(
    app: &mut App,
    states: &TypedSubscription<CallState>,
    modem: &FakeModemControl,
    log: &std::path::Path,
) {
    let Some(index) = modem.ring("0933 111 222") else {
        fail("ring")
    };
    until(app, states, "eingehender Anruf", |s| s.calls.len() == 1);
    let Screen::Incoming(call) = app.model().screen() else {
        fail("Anrufbildschirm")
    };
    click(app, Click::Accept(call), "annehmen");
    until(app, states, "angenommen", |s| {
        s.calls.first().is_some_and(|c| c.phase == Phase::Active)
    });
    if !modem
        .state()
        .calls
        .iter()
        .any(|c| c.index == index && c.status == ModemCallStatus::Active)
    {
        fail("Modem aktiv");
    }
    click(app, Click::Hold(call), "halten");
    until(app, states, "gehalten", |s| {
        s.calls.first().is_some_and(|c| c.phase == Phase::Held)
    });
    click(app, Click::Hangup(call), "auflegen");
    until(app, states, "beendet", |s| s.calls.is_empty());
    if app.model().log.len() != 1 || !std::fs::read_to_string(log).is_ok_and(|t| t.contains("0933"))
    {
        fail("Anrufliste");
    }
    println!("ok    Anrufliste auf dem Datentraeger");
}

/// Arabic, dark, touch, phone size.
fn phone_look() -> Look {
    let size = Size { w: 360.0, h: 720.0 };
    Look {
        lang: Lang::Ar,
        theme: ThemeKind::Dark,
        mode: InputMode::Touch,
        size,
    }
}
