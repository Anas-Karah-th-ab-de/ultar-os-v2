//! Dienstverwaltung: Start in Abhängigkeitsreihenfolge, Überwachung,
//! Neustart mit Backoff, cgroup-Isolation pro Dienst.

use crate::config::{Restart, ServiceConfig};
use crate::log;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const COMPAT_BIN: &str = "/usr/bin/ultar-compat";
const CGROOT: &str = "/sys/fs/cgroup/ultar";

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Pending,
    Running { pid: i32, since: Instant },
    Backoff { until: Instant },
    Done { ok: bool },
    Stopped,
    Failed(String),
}

pub struct Service {
    pub cfg: ServiceConfig,
    pub state: State,
    pub restarts: u32,
    pub want_stop: bool,
    restart_after_stop: bool,
    stop_deadline: Option<Instant>,
}

pub struct Supervisor {
    pub services: BTreeMap<String, Service>,
    pid_map: BTreeMap<i32, String>,
    #[allow(dead_code)]
    pub is_pid1: bool,
}

impl Supervisor {
    pub fn new(cfgs: Vec<ServiceConfig>, is_pid1: bool) -> Self {
        let mut services = BTreeMap::new();
        for c in cfgs {
            let state = if c.manual {
                State::Stopped
            } else {
                State::Pending
            };
            services.insert(
                c.name.clone(),
                Service {
                    cfg: c,
                    state,
                    restarts: 0,
                    want_stop: false,
                    restart_after_stop: false,
                    stop_deadline: None,
                },
            );
        }
        Self {
            services,
            pid_map: BTreeMap::new(),
            is_pid1,
        }
    }

    /// Neu hinzugekommene Dienstdateien aufnehmen (z. B. nach einer App-Installation).
    pub fn reload(&mut self, cfgs: Vec<ServiceConfig>) -> usize {
        let mut n = 0;
        for c in cfgs {
            if !self.services.contains_key(&c.name) {
                let state = if c.manual {
                    State::Stopped
                } else {
                    State::Pending
                };
                self.services.insert(
                    c.name.clone(),
                    Service {
                        cfg: c,
                        state,
                        restarts: 0,
                        want_stop: false,
                        restart_after_stop: false,
                        stop_deadline: None,
                    },
                );
                n += 1;
            }
        }
        n
    }

    fn deps_ready(&self, name: &str) -> bool {
        let s = &self.services[name];
        s.cfg.after.iter().all(|d| match self.services.get(d) {
            None => true, // unbekannte Abhängigkeit blockiert nicht
            Some(dep) => match &dep.state {
                State::Running { .. } => !dep.cfg.oneshot,
                State::Done { ok } => *ok,
                _ => false,
            },
        })
    }

    /// Alles starten, was startbereit ist. Liefert true, wenn alle
    /// automatischen Dienste einen stabilen Zustand erreicht haben.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let names: Vec<String> = self.services.keys().cloned().collect();
        for n in &names {
            let st = self.services[n].state.clone();
            match st {
                State::Pending if self.deps_ready(n) => self.spawn(n),
                State::Backoff { until } if until <= now => self.spawn(n),
                State::Running { .. } => {
                    let s = self.services.get_mut(n).unwrap();
                    if let Some(dl) = s.stop_deadline {
                        if dl <= now {
                            log(&format!("{n}: reagiert nicht auf SIGTERM -> cgroup.kill"));
                            cgroup_kill(n);
                            s.stop_deadline = None;
                        }
                    }
                }
                _ => {}
            }
        }
        self.services
            .values()
            .all(|s| !matches!(s.state, State::Pending | State::Backoff { .. }))
    }

    /// Nächster Zeitpunkt, zu dem tick() etwas tun muss (für poll-Timeout).
    pub fn next_deadline(&self) -> Option<Instant> {
        self.services
            .values()
            .filter_map(|s| match s.state {
                State::Backoff { until } => Some(until),
                State::Running { .. } => s.stop_deadline,
                _ => None,
            })
            .min()
    }

    fn spawn(&mut self, name: &str) {
        let s = self.services.get_mut(name).unwrap();
        s.want_stop = false;
        s.stop_deadline = None;
        let c = &s.cfg;

        let (prog, args): (String, Vec<String>) = if c.compat {
            let mut a = vec!["run".into(), "--".into(), c.exec.clone()];
            a.extend(c.args.iter().cloned());
            (COMPAT_BIN.into(), a)
        } else {
            (c.exec.clone(), c.args.clone())
        };

        let mut cmd = Command::new(&prog);
        cmd.args(&args)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", "/root")
            .env("ULTAR_SERVICE", name)
            .env(
                "ULTAR_EDITION",
                ultar_hal_system::current_edition().as_str(),
            );
        for (k, v) in &c.env {
            cmd.env(k, v);
        }
        if let Some(w) = &c.workdir {
            cmd.current_dir(w);
        }
        if c.console {
            cmd.stdin(open_or_null("/dev/console", false))
                .stdout(open_or_null("/dev/console", true))
                .stderr(open_or_null("/dev/console", true));
        } else {
            let lp = format!("/run/ultar/log/{name}.log");
            let f = OpenOptions::new().create(true).append(true).open(&lp);
            match f {
                Ok(f) => {
                    let f2 = f.try_clone().ok();
                    cmd.stdin(Stdio::null()).stdout(f);
                    if let Some(f2) = f2 {
                        cmd.stderr(f2);
                    }
                }
                Err(_) => {
                    cmd.stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                }
            }
        }

        // cgroup vorbereiten; Datei VOR fork öffnen, im Kind nur write() (async-signal-safe).
        let cg = prepare_cgroup(name, &c.cgroup);
        let console = c.console;
        // Internet-Regeln: steht der Dienst auf der Erlaubt-Liste, läuft er mit der Netz-Gruppe und darf
        // auch bei „Gerät offline“ hinaus (siehe zelle netz erlauben)
        let netz = netz_erlaubt(name);
        unsafe {
            cmd.pre_exec(move || {
                if let Some(f) = &cg {
                    let _ = (&*f).write_all(b"0");
                }
                nix::unistd::setsid().map_err(std::io::Error::from)?;
                // nftables prüft die Gruppe des Sockets (= Haupt-Gruppe des Prozesses)
                if netz {
                    nix::libc::setgid(NETZ_GID);
                }
                if console {
                    // Konsole als steuerndes Terminal -> Strg+C funktioniert
                    extern "C" {
                        fn ioctl(fd: i32, req: u64, ...) -> i32;
                    }
                    ioctl(0, 0x540E /*TIOCSCTTY*/, 1);
                }
                // Signale von init nicht vererben
                let empty = nix::sys::signal::SigSet::empty();
                let _ = empty.thread_set_mask();
                Ok(())
            });
        }

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id() as i32;
                std::mem::forget(child); // Reaping erfolgt zentral über waitpid(-1)
                log(&format!("gestartet: {name} (pid {pid})"));
                s.state = State::Running {
                    pid,
                    since: Instant::now(),
                };
                self.pid_map.insert(pid, name.to_string());
            }
            Err(e) => {
                log(&format!("{name}: Start fehlgeschlagen: {e}"));
                s.restarts += 1;
                s.state = if c.restart == Restart::Never || s.restarts > 10 {
                    State::Failed(e.to_string())
                } else {
                    State::Backoff {
                        until: Instant::now() + backoff(s.restarts),
                    }
                };
            }
        }
    }

    /// Von waitpid gemeldetes Prozessende verarbeiten.
    pub fn on_exit(&mut self, pid: i32, code: i32) {
        let Some(name) = self.pid_map.remove(&pid) else {
            return;
        }; // verwaister Prozess
        let s = self.services.get_mut(&name).unwrap();
        let ran = match s.state {
            State::Running { since, .. } => since.elapsed(),
            _ => Duration::ZERO,
        };
        let ok = code == 0;
        log(&format!(
            "beendet: {name} (pid {pid}, code {code}, Laufzeit {} ms)",
            ran.as_millis()
        ));
        // Restprozesse der Dienst-cgroup beseitigen
        cgroup_kill(&name);
        if s.want_stop {
            s.state = if std::mem::take(&mut s.restart_after_stop) {
                State::Pending
            } else {
                State::Stopped
            };
            return;
        }
        if s.cfg.oneshot {
            s.state = State::Done { ok };
            return;
        }
        let restart = match s.cfg.restart {
            Restart::Always => true,
            Restart::OnFailure => !ok,
            Restart::Never => false,
        };
        if !restart {
            s.state = State::Done { ok };
            return;
        }
        if ran > Duration::from_secs(10) {
            s.restarts = 0;
        }
        s.restarts += 1;
        let d = backoff(s.restarts);
        log(&format!(
            "{name}: Neustart in {} ms (Versuch {})",
            d.as_millis(),
            s.restarts
        ));
        s.state = State::Backoff {
            until: Instant::now() + d,
        };
    }

    pub fn start(&mut self, name: &str) -> Result<(), String> {
        let s = self.services.get_mut(name).ok_or("unbekannter Dienst")?;
        if matches!(s.state, State::Running { .. }) {
            return Err("läuft bereits".into());
        }
        s.restarts = 0;
        s.state = State::Pending;
        Ok(())
    }

    pub fn stop(&mut self, name: &str, grace: Duration) -> Result<(), String> {
        let s = self.services.get_mut(name).ok_or("unbekannter Dienst")?;
        s.want_stop = true;
        match s.state {
            State::Running { pid, .. } => {
                let _ = kill(Pid::from_raw(-pid), Signal::SIGTERM);
                s.stop_deadline = Some(Instant::now() + grace);
            }
            _ => s.state = State::Stopped,
        }
        Ok(())
    }

    pub fn restart(&mut self, name: &str, grace: Duration) -> Result<(), String> {
        let running = matches!(
            self.services.get(name).ok_or("unbekannter Dienst")?.state,
            State::Running { .. }
        );
        if running {
            self.stop(name, grace)?;
            self.services.get_mut(name).unwrap().restart_after_stop = true;
        } else {
            self.start(name)?;
        }
        Ok(())
    }

    pub fn running_count(&self) -> usize {
        self.services
            .values()
            .filter(|s| matches!(s.state, State::Running { .. }))
            .count()
    }

    pub fn stop_all(&mut self) {
        let names: Vec<String> = self.services.keys().cloned().collect();
        for n in names {
            let _ = self.stop(&n, Duration::from_secs(3600));
        }
    }

    pub fn status(&self) -> String {
        let mut out = format!(
            "{:<20} {:<10} {:>7} {:>8} {:>10}\n",
            "DIENST", "ZUSTAND", "PID", "NEUSTART", "LAUFZEIT"
        );
        for (n, s) in &self.services {
            let (st, pid, up) = match &s.state {
                State::Pending => ("wartet".to_string(), "-".to_string(), "-".to_string()),
                State::Running { pid, since } => (
                    "läuft".into(),
                    pid.to_string(),
                    format!("{}s", since.elapsed().as_secs()),
                ),
                State::Backoff { .. } => ("backoff".into(), "-".into(), "-".into()),
                State::Done { ok: true } => ("fertig".into(), "-".into(), "-".into()),
                State::Done { ok: false } => ("fehler".into(), "-".into(), "-".into()),
                State::Stopped => ("gestoppt".into(), "-".into(), "-".into()),
                State::Failed(_) => ("defekt".into(), "-".into(), "-".into()),
            };
            out += &format!(
                "{:<20} {:<10} {:>7} {:>8} {:>10}\n",
                n, st, pid, s.restarts, up
            );
        }
        out
    }
}

fn backoff(n: u32) -> Duration {
    Duration::from_millis((100u64 << n.min(9)).min(30_000))
}

fn open_or_null(p: &str, write: bool) -> Stdio {
    match OpenOptions::new().read(!write).write(write).open(p) {
        Ok(f) => Stdio::from(f),
        Err(_) => Stdio::null(),
    }
}

fn prepare_cgroup(name: &str, limits: &BTreeMap<String, String>) -> Option<fs::File> {
    let dir = format!("{CGROOT}/{name}");
    fs::create_dir_all(&dir).ok()?;
    for (k, v) in limits {
        if let Err(e) = fs::write(format!("{dir}/{k}"), v) {
            log(&format!("{name}: cgroup {k}={v} nicht gesetzt: {e}"));
        }
    }
    OpenOptions::new()
        .write(true)
        .open(format!("{dir}/cgroup.procs"))
        .ok()
}

fn cgroup_kill(name: &str) {
    let _ = fs::write(format!("{CGROOT}/{name}/cgroup.kill"), "1");
}

/// Gruppe, deren Prozesse trotz „Gerät offline“ ins Internet dürfen (wie ultar_zellen::regeln::NETZ_GID)
const NETZ_GID: u32 = 47900;

fn netz_erlaubt(name: &str) -> bool {
    std::fs::read_to_string("/data/ultar/netz-erlaubt")
        .map(|s| s.lines().any(|l| l.trim() == name))
        .unwrap_or(false)
}
