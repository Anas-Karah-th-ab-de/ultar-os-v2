//! ultar-init – PID 1 von Ultar OS (KM Code Craft).
//!
//! Ereignisschleife: signalfd (SIGCHLD/Shutdown) + Steuer-Socket, kein Polling.
//! Läuft auch ohne PID 1 (Entwicklungsmodus) – dann ohne Mounts/Reboot.

mod boot;
mod config;
mod diag;
mod fwhelper;
mod hw;
mod rescue;
mod supervisor;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};
use supervisor::Supervisor;

pub fn log(msg: &str) {
    let line = format!("[ultar-init {:>8.3}] {msg}\n", uptime());
    let _ = std::io::stderr().write_all(line.as_bytes());
    diag::remember(&line);
}

fn uptime() -> f64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok()))
        .unwrap_or(0.0)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Halt {
    PowerOff,
    Reboot,
}

fn sock_path() -> String {
    std::env::var("ULTAR_CTL_SOCK").unwrap_or_else(|_| "/run/ultar/ctl.sock".into())
}

fn main() {
    let is_pid1 = std::process::id() == 1;
    if is_pid1 {
        // PID 1 darf nie enden (sonst Kernel-Panik und Neustart-Schleife): bei einem Programmfehler stehen bleiben,
        // damit die USB-Diagnose weiter erreichbar ist.
        std::panic::set_hook(Box::new(|info| {
            log(&format!("PROGRAMMFEHLER in ultar-init: {info}"));
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }));
        boot::early_mounts();
        // Samsung-Firmware (/vendor) nur auf Handys – auf PCs gibt es die Partition nicht (oder sie ist fremd)
        if ultar_hal_system::current_edition() == ultar_profile::Edition::Phone {
            boot::mount_vendor_firmware();
        }
        fwhelper::start();
        boot::boot_marker();
        let _ = nix::sys::reboot::set_cad_enabled(false); // Strg+Alt+Entf -> SIGINT
    }
    log(&format!(
        "Ultar OS init {} startet (pid1={is_pid1}, Ausgabe {})",
        env!("CARGO_PKG_VERSION"),
        ultar_hal_system::current_edition()
    ));
    let cfg = config::load_init();
    if is_pid1 {
        boot::configure(&cfg);
        if ultar_hal_system::current_edition() != ultar_profile::Edition::Phone {
            boot::sprache_von_startpartition();
        }
        diag::start();
        // /data erst jetzt einbinden (Watchdog läuft) und im Hintergrund, damit der
        // langsame ext4-Mount den Boot nicht blockiert.
        // Nur Handy: größte eMMC-Partition als /data. Auf PCs/Laptops (auch mit eMMC) NIE eine fremde
        // Partition einbinden – dort kommt /data nur aus der Partition „ultar-data“ (init.toml).
        if ultar_hal_system::current_edition() == ultar_profile::Edition::Phone {
            boot::mount_userdata_bg();
        }
    }

    // Signale blockieren und über signalfd empfangen
    let mut mask = SigSet::empty();
    for s in [
        Signal::SIGCHLD,
        Signal::SIGTERM,
        Signal::SIGINT,
        Signal::SIGUSR1,
        Signal::SIGUSR2,
        Signal::SIGPWR,
        Signal::SIGHUP,
    ] {
        mask.add(s);
    }
    mask.thread_block().expect("sigmask");
    let mut sfd = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
        .expect("signalfd");

    let sp = sock_path();
    let _ = std::fs::remove_file(&sp);
    if let Some(d) = std::path::Path::new(&sp).parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let listener = match UnixListener::bind(&sp) {
        Ok(l) => {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&sp, std::fs::Permissions::from_mode(0o600));
            l.set_nonblocking(true).ok();
            Some(l)
        }
        Err(e) => {
            log(&format!("Steuer-Socket {sp} nicht verfügbar: {e}"));
            None
        }
    };

    let services = config::load_services();
    log(&format!("{} Dienste geladen", services.len()));
    let mut sup = Supervisor::new(services, is_pid1);
    let grace = Duration::from_millis(cfg.shutdown_grace_ms);
    let mut ready_reported = false;
    let mut halt: Option<Halt> = None;

    loop {
        let all_settled = sup.tick();
        if all_settled && !ready_reported {
            ready_reported = true;
            log(&format!(
                "System bereit nach {:.0} ms seit Kernelstart",
                uptime() * 1000.0
            ));
        }
        if halt.is_some() {
            break;
        }

        // Poll NIE unendlich: der Reaper darf sich nicht allein auf signalfd verlassen. Ein
        // Kindtod, dessen SIGCHLD der Door-Thread (cmd_server) mit seinem eigenen waitpid
        // „verschluckt" oder der mit signalfd-Verarbeitung kollidiert, würde die Schleife sonst
        // ewig schlafen lassen → Dienste bleiben Zombies und starten nicht neu. Darum max. 1 s,
        // und nach JEDEM Aufwachen zusätzlich reapen (WNOHANG ist billig).
        const MAX_IDLE_MS: i32 = 1000;
        let timeout = match sup.next_deadline() {
            Some(t) => PollTimeout::try_from(
                (t.saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(MAX_IDLE_MS as u128) as i32)
                    .max(1),
            )
            .unwrap_or(PollTimeout::from(MAX_IDLE_MS as u16)),
            None => PollTimeout::from(MAX_IDLE_MS as u16),
        };
        let mut fds = vec![PollFd::new(sfd.as_fd(), PollFlags::POLLIN)];
        if let Some(l) = &listener {
            fds.push(PollFd::new(l.as_fd(), PollFlags::POLLIN));
        }
        match poll(&mut fds, timeout) {
            Ok(_) | Err(nix::errno::Errno::EINTR) => {}
            Err(e) => log(&format!("poll: {e}")),
        }
        let sig_ready = fds[0]
            .revents()
            .is_some_and(|r| r.contains(PollFlags::POLLIN));
        let ctl_ready = fds
            .get(1)
            .and_then(|f| f.revents())
            .is_some_and(|r| r.contains(PollFlags::POLLIN));
        drop(fds);

        if sig_ready {
            while let Ok(Some(si)) = sfd.read_signal() {
                match Signal::try_from(si.ssi_signo as i32) {
                    Ok(Signal::SIGCHLD) => {}
                    Ok(Signal::SIGUSR2) | Ok(Signal::SIGPWR) => halt = Some(Halt::PowerOff),
                    Ok(Signal::SIGTERM) | Ok(Signal::SIGINT) => halt = Some(Halt::Reboot),
                    Ok(s) => log(&format!("Signal {s:?} ignoriert")),
                    Err(_) => {}
                }
            }
        }
        // Immer reapen (auch bei Timeout-Aufwachen), damit ein verpasstes SIGCHLD die Dienste
        // spätestens nach MAX_IDLE_MS wieder einsammelt und neu startet.
        reap(&mut sup);
        if ctl_ready {
            if let Some(l) = &listener {
                while let Ok((stream, _)) = l.accept() {
                    if let Some(h) = handle_ctl(stream, &mut sup, grace) {
                        halt = Some(h);
                    }
                }
            }
        }
    }

    shutdown(&mut sup, &mut sfd, grace, halt.unwrap(), is_pid1);
}

fn reap(sup: &mut Supervisor) {
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, code)) => sup.on_exit(pid.as_raw(), code),
            Ok(WaitStatus::Signaled(pid, sig, _)) => sup.on_exit(pid.as_raw(), 128 + sig as i32),
            Ok(WaitStatus::StillAlive) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

fn handle_ctl(
    stream: std::os::unix::net::UnixStream,
    sup: &mut Supervisor,
    grace: Duration,
) -> Option<Halt> {
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok();
    let mut line = String::new();
    let mut r = BufReader::new(&stream);
    if r.read_line(&mut line).is_err() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("");
    let mut halt = None;
    let reply = match cmd {
        "status" => sup.status(),
        "start" => sup.start(arg).map(|_| format!("{arg}: Start angefordert\n")).unwrap_or_else(|e| format!("Fehler: {e}\n")),
        "stop" => sup.stop(arg, grace).map(|_| format!("{arg}: gestoppt\n")).unwrap_or_else(|e| format!("Fehler: {e}\n")),
        "restart" => sup.restart(arg, grace).map(|_| format!("{arg}: Neustart angefordert\n")).unwrap_or_else(|e| format!("Fehler: {e}\n")),
        "reload" => format!("{} neue Dienste\n", sup.reload(config::load_services())),
        "uptime" => format!("{:.2}\n", uptime()),
        "poweroff" => { halt = Some(Halt::PowerOff); "Fahre herunter …\n".into() }
        "reboot" => { halt = Some(Halt::Reboot); "Starte neu …\n".into() }
        // Notausstieg: 1 = Oberfläche, 2 = alle Dienste, 3 = Neustart (sauber über den Supervisor)
        "rescue" => match rescue::Level::from_arg(arg) {
            Some(rescue::Level::Reboot) => { halt = Some(Halt::Reboot); "Notausstieg: Neustart …\n".into() }
            Some(l) => rescue::act(l, "ultar-ctl", "none"),
            None => "rescue 1|2|3  (1 = Oberfläche, 2 = alle Dienste, 3 = Neustart)\n".into(),
        },
        _ => "Befehle: status | start <d> | stop <d> | restart <d> | reload | uptime | poweroff | reboot | rescue 1|2|3\n".into(),
    };
    let _ = (&stream).write_all(reply.as_bytes());
    halt
}

fn shutdown(sup: &mut Supervisor, sfd: &mut SignalFd, grace: Duration, halt: Halt, is_pid1: bool) {
    log(&format!("Herunterfahren: {halt:?}"));
    sup.stop_all();
    let deadline = Instant::now() + grace;
    while sup.running_count() > 0 && Instant::now() < deadline {
        let mut fds = [PollFd::new(sfd.as_fd(), PollFlags::POLLIN)];
        let _ = poll(&mut fds, PollTimeout::from(50u8));
        while let Ok(Some(_)) = sfd.read_signal() {}
        reap(sup);
    }
    if !is_pid1 {
        log("Entwicklungsmodus: Ende");
        return;
    }
    // Alles Übrige hart beenden (kill(-1) trifft alle außer PID 1)
    let _ = nix::sys::signal::kill(Pid::from_raw(-1), Signal::SIGKILL);
    std::thread::sleep(Duration::from_millis(100));
    reap(sup);
    boot::unmount_all();
    log("Auf Wiedersehen.");
    let how = match halt {
        Halt::PowerOff => nix::sys::reboot::RebootMode::RB_POWER_OFF,
        Halt::Reboot => nix::sys::reboot::RebootMode::RB_AUTOBOOT,
    };
    let _ = nix::sys::reboot::reboot(how);
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
