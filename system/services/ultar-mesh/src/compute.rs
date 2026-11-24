//! Pooling computing power: one device hands a list of jobs to the devices that lend
//! theirs and collects exactly one result per job.
//!
//! [`Scheduler`] is the asking side (no input or output of its own, like the node):
//! a job nobody reports on for [`SILENCE`] goes back into the queue and its device gets a
//! strike, [`MAX_STRIKES`] strikes take a device out, a job that runs longer than
//! [`STRAGGLER`] with nothing else to do is started a second time, the first result
//! wins, and when no other device is left this device does the rest itself.
//! [`Memory`] is the lending side: a job id asked twice is answered from memory.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// A running job without a sign of life for this long is taken as lost.
pub const SILENCE: Duration = Duration::from_secs(8);
/// A job running this long is started a second time when a device is free.
pub const STRAGGLER: Duration = Duration::from_secs(3);
/// Lost or refused jobs in a row that take a device out of the run.
pub const MAX_STRIKES: u8 = 4;
/// The name of this device as a worker.
pub const LOCAL: &str = "";

/// One job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// Its id.
    pub id: String,
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

/// How a job ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Exit code (`None`: it could not run).
    pub code: Option<i32>,
    /// What it printed.
    pub output: String,
    /// The device that delivered it ([`LOCAL`]: this one).
    pub by: String,
}

struct Run {
    job: usize,
    worker: String,
    since: Instant,
    alive: Instant,
}

#[derive(Default)]
struct Worker {
    strikes: u8,
    out: bool,
}

/// The asking side of a run.
pub struct Scheduler {
    jobs: Vec<Job>,
    queue: VecDeque<usize>,
    runs: Vec<Run>,
    results: BTreeMap<usize, Outcome>,
    workers: BTreeMap<String, Worker>,
}

impl Scheduler {
    /// A run of `jobs` on `workers` (device ids); this device is the reserve.
    pub fn new(jobs: Vec<Job>, workers: &[String]) -> Self {
        let mut all: BTreeMap<String, Worker> = workers
            .iter()
            .map(|w| (w.clone(), Worker::default()))
            .collect();
        all.insert(LOCAL.to_owned(), Worker::default());
        Self {
            queue: (0..jobs.len()).collect(),
            jobs,
            runs: Vec::new(),
            results: BTreeMap::new(),
            workers: all,
        }
    }

    /// `true` once every job has its result.
    pub fn finished(&self) -> bool {
        self.results.len() == self.jobs.len()
    }

    /// The results in the order of the jobs (`None`: not there yet).
    pub fn results(&self) -> Vec<Option<&Outcome>> {
        (0..self.jobs.len()).map(|i| self.results.get(&i)).collect()
    }

    fn free(&self) -> Vec<String> {
        let others = self.workers.iter().any(|(id, w)| id != LOCAL && !w.out);
        self.workers
            .iter()
            .filter(|(id, w)| !w.out && (id.as_str() != LOCAL || !others))
            .filter(|(id, _)| !self.runs.iter().any(|r| &r.worker == *id))
            .map(|(id, _)| id.clone())
            .collect()
    }

    fn strike(&mut self, worker: &str) {
        if let Some(w) = self.workers.get_mut(worker) {
            w.strikes += 1;
            w.out = w.strikes >= MAX_STRIKES && worker != LOCAL;
        }
    }

    fn give_back(&mut self, at: usize) {
        let run = self.runs.remove(at);
        let waiting = self.queue.contains(&run.job) || self.runs.iter().any(|r| r.job == run.job);
        if !self.results.contains_key(&run.job) && !waiting {
            self.queue.push_front(run.job);
        }
    }

    /// What to start now: `(worker, job)` pairs.
    pub fn step(&mut self, now: Instant) -> Vec<(String, Job)> {
        while let Some(at) = self.runs.iter().position(|r| now >= r.alive + SILENCE) {
            let worker = self.runs[at].worker.clone();
            self.strike(&worker);
            self.give_back(at);
        }
        let mut orders = Vec::new();
        for worker in self.free() {
            let job = self.queue.pop_front().or_else(|| {
                self.runs
                    .iter()
                    .filter(|r| now >= r.since + STRAGGLER)
                    .map(|r| r.job)
                    .find(|j| self.runs.iter().filter(|r| r.job == *j).count() == 1)
            });
            let Some(job) = job else {
                break;
            };
            self.runs.push(Run {
                job,
                worker: worker.clone(),
                since: now,
                alive: now,
            });
            orders.push((worker, self.jobs[job].clone()));
        }
        orders
    }

    fn run_of(&self, worker: &str, id: &str) -> Option<usize> {
        self.runs
            .iter()
            .position(|r| r.worker == worker && self.jobs[r.job].id == id)
    }

    /// `worker` says the job `id` is still running.
    pub fn alive(&mut self, worker: &str, id: &str, now: Instant) {
        if let Some(at) = self.run_of(worker, id) {
            self.runs[at].alive = now;
        }
    }

    /// `worker` delivered a result; the first one per job wins.
    pub fn done(&mut self, worker: &str, id: &str, code: Option<i32>, output: String) {
        let Some(at) = self.run_of(worker, id) else {
            return;
        };
        let job = self.runs.remove(at).job;
        if let Some(w) = self.workers.get_mut(worker) {
            w.strikes = 0;
        }
        self.results.entry(job).or_insert(Outcome {
            code,
            output,
            by: worker.to_owned(),
        });
    }

    /// `worker` refused the job `id` (busy, or not allowed).
    pub fn refused(&mut self, worker: &str, id: &str) {
        if let Some(at) = self.run_of(worker, id) {
            self.strike(worker);
            self.give_back(at);
        }
    }

    /// `worker` is gone for good.
    pub fn gone(&mut self, worker: &str) {
        if worker == LOCAL {
            return;
        }
        if let Some(w) = self.workers.get_mut(worker) {
            w.out = true;
        }
        while let Some(at) = self.runs.iter().position(|r| r.worker == worker) {
            self.give_back(at);
        }
    }
}

/// What the lending side remembers, so a job asked twice runs once.
#[derive(Default)]
pub struct Memory {
    done: BTreeMap<(String, String), (Option<i32>, String)>,
    order: VecDeque<(String, String)>,
}

/// Results a lending device remembers.
pub const REMEMBERED: usize = 256;

impl Memory {
    /// The stored result of job `id` from `peer`.
    pub fn get(&self, peer: &str, id: &str) -> Option<&(Option<i32>, String)> {
        self.done.get(&(peer.to_owned(), id.to_owned()))
    }

    /// Stores a result (the oldest is dropped beyond [`REMEMBERED`]).
    pub fn put(&mut self, peer: &str, id: &str, code: Option<i32>, output: String) {
        let key = (peer.to_owned(), id.to_owned());
        if self.done.insert(key.clone(), (code, output)).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > REMEMBERED {
            if let Some(old) = self.order.pop_front() {
                self.done.remove(&old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jobs(n: usize) -> Vec<Job> {
        (0..n)
            .map(|i| Job {
                id: format!("j{i}"),
                program: "work".into(),
                args: vec![i.to_string()],
            })
            .collect()
    }

    fn finish(s: &mut Scheduler, orders: &[(String, Job)]) {
        for (worker, job) in orders {
            s.done(worker, &job.id, Some(0), job.args[0].clone());
        }
    }

    #[test]
    fn every_job_gets_exactly_one_result() {
        let mut s = Scheduler::new(jobs(7), &["a".into(), "b".into()]);
        let now = Instant::now();
        let mut rounds = 0;
        while !s.finished() {
            let orders = s.step(now);
            assert!(orders.iter().all(|(w, _)| w != LOCAL), "others are there");
            finish(&mut s, &orders);
            rounds += 1;
            assert!(rounds < 20);
        }
        let outputs: Vec<&str> = s
            .results()
            .iter()
            .map(|r| r.unwrap().output.as_str())
            .collect();
        assert_eq!(outputs, ["0", "1", "2", "3", "4", "5", "6"]);
    }

    #[test]
    fn a_silent_device_loses_its_job_and_after_four_times_its_place() {
        let mut s = Scheduler::new(jobs(1), &["a".into()]);
        let mut now = Instant::now();
        for _ in 0..MAX_STRIKES {
            let orders = s.step(now);
            assert_eq!(orders[0].0, "a");
            now += SILENCE;
        }
        // "a" is out: this device does the rest itself.
        let orders = s.step(now);
        assert_eq!(orders[0].0, LOCAL);
        finish(&mut s, &orders);
        assert!(s.finished());
        assert_eq!(s.results()[0].unwrap().by, LOCAL);
    }

    #[test]
    fn a_sign_of_life_keeps_a_long_job() {
        let mut s = Scheduler::new(jobs(1), &["a".into()]);
        let mut now = Instant::now();
        let orders = s.step(now);
        for _ in 0..5 {
            now += SILENCE - Duration::from_secs(1);
            s.alive("a", "j0", now);
            assert!(s.step(now).is_empty());
        }
        finish(&mut s, &orders);
        assert!(s.finished());
    }

    #[test]
    fn a_straggler_is_started_twice_and_the_first_result_wins() {
        let mut s = Scheduler::new(jobs(1), &["slow".into(), "fast".into()]);
        let now = Instant::now();
        let first = s.step(now);
        assert_eq!(first.len(), 1);
        let later = now + STRAGGLER;
        s.alive(&first[0].0, "j0", later);
        let second = s.step(later);
        assert_eq!(second.len(), 1);
        assert_ne!(second[0].0, first[0].0);
        s.done(&second[0].0, "j0", Some(0), "fast".into());
        s.done(&first[0].0, "j0", Some(0), "slow".into());
        assert_eq!(s.results()[0].unwrap().output, "fast");
        assert!(s.step(later + SILENCE).is_empty());
    }

    #[test]
    fn a_device_that_leaves_gives_its_job_back() {
        let mut s = Scheduler::new(jobs(2), &["a".into(), "b".into()]);
        let now = Instant::now();
        let orders = s.step(now);
        assert_eq!(orders.len(), 2);
        s.gone("a");
        s.refused("b", &orders[1].1.id);
        let again = s.step(now);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].0, "b");
    }

    #[test]
    fn a_job_asked_twice_is_answered_from_memory() {
        let mut m = Memory::default();
        assert!(m.get("p", "j").is_none());
        m.put("p", "j", Some(0), "out".into());
        assert_eq!(m.get("p", "j"), Some(&(Some(0), "out".to_owned())));
        assert!(m.get("q", "j").is_none());
        for i in 0..REMEMBERED {
            m.put("p", &format!("x{i}"), None, String::new());
        }
        assert!(m.get("p", "j").is_none());
    }
}
