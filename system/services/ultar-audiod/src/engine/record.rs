//! The microphone side of the engine: who records, and one round of reading the card and
//! handing the microphone (and the echo-cancelled copy) to every recorder.

use super::{Engine, Event, CHUNK};
use crate::capture::mono_of;
use crate::conn::{Conn, Phase};
use crate::resample::to_i16;
use std::time::Instant;

impl Engine {
    pub(super) fn recorders(&self) -> impl Iterator<Item = &Conn> {
        self.conns.iter().filter(|c| c.records())
    }

    pub(super) fn record(&mut self, far: Vec<f32>) {
        let any = self.recorders().next().is_some() || !self.echo.bus_users.is_empty();
        if !any {
            self.card.fresh = true;
            return;
        }
        let mut lost = false;
        let mut mic48 = Vec::new();
        if let Some(dev) = self.card.device.as_mut() {
            let mut buf = vec![0i16; 9600];
            if self.card.fresh {
                // what piled up while nobody listened is old: it is thrown away
                for _ in 0..64 {
                    if !matches!(dev.read(&mut buf), Ok(n) if n > 0) {
                        break;
                    }
                }
                self.card.fresh = false;
            }
            for _ in 0..4 {
                match dev.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => mic48.extend(mono_of(&buf[..n])),
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
        }
        if lost {
            self.lose_card(self.card.last.unwrap_or_else(Instant::now));
            return;
        }
        let echo = self.recorders().any(|c| {
            matches!(&c.phase, Phase::Open(o) if o.rec.as_ref().is_some_and(|r| r.wants_echo_path()))
        });
        let (mut m16, mut f16) = (Vec::new(), Vec::new());
        if echo || !self.echo.bus_users.is_empty() {
            self.echo.mic16.process(&mic48, &mut m16);
        }
        if echo {
            self.echo.far16.process(&far, &mut f16);
        }
        for c in &mut self.conns {
            if let Phase::Open(o) = &mut c.phase {
                if let Some(rec) = o.rec.as_mut() {
                    rec.feed(&mic48, &m16, &f16);
                }
            }
        }
        if !self.echo.bus_users.is_empty() {
            self.echo.bus_acc.extend(to_i16(&m16));
            while self.echo.bus_acc.len() >= CHUNK {
                let chunk: Vec<i16> = self.echo.bus_acc.drain(..CHUNK).collect();
                self.events.push(Event::Chunk(chunk));
            }
        }
    }
}
