//! Service calls that may take longer than a turn of the shell may wait (`net.networks`
//! up to 15 s, `net.join` up to 30 s, `vpn.set`): each runs on its own worker thread and its
//! answer comes back through a channel that the loop looks at every turn. At most
//! [`MAX_RUNNING`] run at once and never two of the same kind.

use crate::model::services::Call;
use crate::ports::bus::BusPorts;
use crate::ports::services::{Answer, ServicesPort};
use crate::ports::PortError;
use std::sync::mpsc::{channel, Receiver, Sender};

/// Most slow calls at once.
pub const MAX_RUNNING: usize = 4;

/// A finished slow call.
pub type Finished = (Call, Result<Answer, PortError>);

/// The slow calls that run.
pub struct SlowCalls {
    tx: Sender<Finished>,
    rx: Receiver<Finished>,
    running: Vec<std::mem::Discriminant<Call>>,
}

impl Default for SlowCalls {
    fn default() -> SlowCalls {
        let (tx, rx) = channel();
        SlowCalls {
            tx,
            rx,
            running: Vec::new(),
        }
    }
}

impl SlowCalls {
    /// Starts `call` on a worker; `false` when one of its kind runs or too many run (the
    /// call is dropped – with it a typed password, which is wiped).
    pub fn start(&mut self, ports: &BusPorts, call: Call) -> bool {
        let kind = std::mem::discriminant(&call);
        if self.running.len() >= MAX_RUNNING || self.running.contains(&kind) {
            return false;
        }
        let ports = ports.clone();
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name("shell-call".into())
            .spawn(move || {
                let answer = ports.call(&call);
                let _ = tx.send((call, answer));
            });
        if spawned.is_ok() {
            self.running.push(kind);
        }
        spawned.is_ok()
    }

    /// The calls that finished since the last look.
    pub fn finished(&mut self) -> Vec<Finished> {
        let done: Vec<Finished> = self.rx.try_iter().collect();
        for (call, _) in &done {
            let kind = std::mem::discriminant(call);
            self.running.retain(|k| *k != kind);
        }
        done
    }

    /// A call of this kind runs.
    pub fn runs(&self, call: &Call) -> bool {
        self.running.contains(&std::mem::discriminant(call))
    }
}
