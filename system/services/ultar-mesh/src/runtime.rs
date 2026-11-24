//! A node on the network: applies what the node wants sent, runs the jobs it lends its
//! computing power to, and drives a run of jobs this device asked for. What is left for
//! the host – asking the person, opening an app, the clipboard – comes back as effects.

use crate::compute::{Job, Memory, Outcome, Scheduler, LOCAL};
use crate::net::NetEvent;
use crate::node::{Effect, Node};
use crate::secure::Sender;
use crate::wire::Frame;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{Capability, Right};

/// Longest output of a job that is sent back.
pub const MAX_OUTPUT: usize = 16 * 1024;
const ALIVE_EVERY: Duration = Duration::from_secs(2);

/// A node with its channels.
pub struct Runtime {
    /// The node.
    pub node: Node,
    senders: BTreeMap<String, (u64, Sender)>,
    events: mpsc::Sender<NetEvent>,
    memory: Memory,
    running: Arc<AtomicUsize>,
    run: Option<Scheduler>,
}

fn cut(mut text: String) -> String {
    if text.len() > MAX_OUTPUT {
        let mut end = MAX_OUTPUT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

fn execute(program: &str, args: &[String]) -> (Option<i32>, String) {
    match std::process::Command::new(program).args(args).output() {
        Ok(out) => (
            out.status.code(),
            cut(String::from_utf8_lossy(&out.stdout).into_owned()),
        ),
        Err(_) => (None, String::new()),
    }
}

impl Runtime {
    /// `node` on the network; `events` is where the network reports to.
    pub fn new(node: Node, events: mpsc::Sender<NetEvent>) -> Self {
        Self {
            node,
            senders: BTreeMap::new(),
            events,
            memory: Memory::default(),
            running: Arc::new(AtomicUsize::new(0)),
            run: None,
        }
    }

    fn send(&self, peer: &str, frame: &Frame) -> bool {
        self.senders.get(peer).is_some_and(|(_, s)| s.send(frame))
    }

    fn start_job(&mut self, peer: &str, id: String, program: String, args: Vec<String>) {
        if let Some((code, output)) = self.memory.get(peer, &id).cloned() {
            let _ = self.send(peer, &Frame::JobDone { id, code, output });
            return;
        }
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        if self.running.fetch_add(1, Ordering::SeqCst) >= cores {
            self.running.fetch_sub(1, Ordering::SeqCst);
            let _ = self.send(peer, &Frame::JobRefused { id });
            return;
        }
        let (events, running) = (self.events.clone(), self.running.clone());
        let sender = self.senders.get(peer).map(|(_, s)| s.clone());
        let peer = peer.to_owned();
        let _ = std::thread::Builder::new().spawn(move || {
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let alive = sender.map(|sender| {
                let id = id.clone();
                std::thread::spawn(move || {
                    let waiting = mpsc::RecvTimeoutError::Timeout;
                    while done_rx.recv_timeout(ALIVE_EVERY) == Err(waiting) {
                        if !sender.send(&Frame::JobAlive { id: id.clone() }) {
                            break;
                        }
                    }
                })
            });
            let (code, output) = execute(&program, &args);
            drop(done_tx);
            if let Some(alive) = alive {
                let _ = alive.join();
            }
            running.fetch_sub(1, Ordering::SeqCst);
            let _ = events.send(NetEvent::Ran {
                peer,
                id,
                code,
                output,
            });
        });
    }

    /// Does what the node wants on the network; returns what is left for the host.
    pub fn apply(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        let mut left = Vec::new();
        for effect in effects {
            match effect {
                Effect::Send(peer, frame) => {
                    let _ = self.send(&peer, &frame);
                }
                Effect::Run {
                    peer,
                    id,
                    program,
                    args,
                } => self.start_job(&peer, id, program, args),
                Effect::Job(peer, frame) => self.on_job_frame(&peer, frame),
                other => left.push(other),
            }
        }
        left
    }

    fn on_job_frame(&mut self, peer: &str, frame: Frame) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        match frame {
            Frame::JobAlive { id } => run.alive(peer, &id, Instant::now()),
            Frame::JobDone { id, code, output } => run.done(peer, &id, code, output),
            Frame::JobRefused { id } => run.refused(peer, &id),
            _ => {}
        }
    }

    /// Something happened on the network; returns what is left for the host.
    pub fn handle(&mut self, event: NetEvent, now: Instant) -> Vec<Effect> {
        let effects = match event {
            NetEvent::Up {
                peer,
                key,
                link,
                sender,
            } => {
                if let Some((_, old)) = self.senders.insert(peer.clone(), (link, sender)) {
                    old.close();
                }
                let effects = self.node.link_up(&peer, &key);
                if effects.is_empty() {
                    if let Some((_, refused)) = self.senders.remove(&peer) {
                        refused.close();
                    }
                }
                effects
            }
            NetEvent::Frame { peer, link, frame } => {
                let current = self.senders.get(&peer).is_some_and(|(l, _)| *l == link);
                if current {
                    self.node.frame(&peer, frame, now)
                } else {
                    Vec::new()
                }
            }
            NetEvent::Down { peer, link } => {
                let current = self.senders.get(&peer).is_some_and(|(l, _)| *l == link);
                if !current {
                    return Vec::new();
                }
                self.senders.remove(&peer);
                if let Some(run) = self.run.as_mut() {
                    run.gone(&peer);
                }
                self.node.link_down(&peer)
            }
            NetEvent::Ran {
                peer,
                id,
                code,
                output,
            } => {
                if peer == LOCAL {
                    if let Some(run) = self.run.as_mut() {
                        run.done(LOCAL, &id, code, output);
                    }
                } else {
                    self.memory.put(&peer, &id, code, output.clone());
                    let _ = self.send(&peer, &Frame::JobDone { id, code, output });
                }
                Vec::new()
            }
        };
        self.apply(effects)
    }

    /// Starts a run of `jobs` on the reachable paired devices that lend computing power
    /// (this device does what is left). `false` while another run is going.
    pub fn compute(&mut self, jobs: Vec<Job>) -> bool {
        if self.run.is_some() {
            return false;
        }
        let workers: Vec<String> = self
            .node
            .state()
            .peers
            .into_iter()
            .filter(|p| p.paired && p.reachable && p.capabilities.contains(&Capability::Compute))
            .map(|p| p.id)
            .collect();
        self.run = Some(Scheduler::new(jobs, &workers));
        true
    }

    /// The results of the run once every job has one (the run is over then).
    pub fn computed(&mut self) -> Option<Vec<Outcome>> {
        if !self.run.as_ref().is_some_and(Scheduler::finished) {
            return None;
        }
        let run = self.run.take()?;
        Some(run.results().into_iter().flatten().cloned().collect())
    }

    /// Regular work; returns what is left for the host.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let orders = self.run.as_mut().map_or_else(Vec::new, |r| r.step(now));
        for (worker, job) in orders {
            if worker == LOCAL {
                self.start_job(LOCAL, job.id, job.program, job.args);
            } else {
                let frame = Frame::Job {
                    id: job.id.clone(),
                    program: job.program,
                    args: job.args,
                };
                if !self.send(&worker, &frame) {
                    if let Some(run) = self.run.as_mut() {
                        run.refused(&worker, &job.id);
                    }
                }
            }
        }
        let effects = self.node.tick(now);
        self.apply(effects)
    }

    /// Switches the mesh on or off; off closes every channel.
    pub fn set_enabled(&mut self, on: bool) -> Vec<Effect> {
        if !on {
            for (_, (_, sender)) in std::mem::take(&mut self.senders) {
                sender.close();
            }
        }
        let effects = self.node.set_enabled(on);
        self.apply(effects)
    }

    /// `true` when `peer` may run jobs here (shown in the settings).
    pub fn lends_to(&self, peer: &str) -> bool {
        self.node
            .state()
            .peers
            .iter()
            .any(|p| p.id == peer && p.paired && p.rights.contains(&Right::Compute))
    }
}
