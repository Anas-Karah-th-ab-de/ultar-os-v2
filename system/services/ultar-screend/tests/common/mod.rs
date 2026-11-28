//! A scripted screen for the tests.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use ultar_screend::screen::{Frame, Screen, ScreenError};

/// What the fake screen shows now (the test changes it between looks).
#[derive(Clone)]
pub struct Shared(pub Arc<Mutex<State>>);

pub struct State {
    pub frame: Frame,
    /// Results to give before showing `frame` (e.g. `Locked`).
    pub script: VecDeque<ScreenError>,
    /// How often the gate was opened.
    pub gates: u32,
}

impl Shared {
    pub fn new(width: u32, height: u32, fill: u8) -> Self {
        let frame = Frame {
            width,
            height,
            data: vec![fill; width as usize * height as usize * 4],
        };
        Self(Arc::new(Mutex::new(State {
            frame,
            script: VecDeque::new(),
            gates: 0,
        })))
    }

    pub fn poke(&self, x: u32, y: u32, v: u8) {
        let mut s = self.0.lock().unwrap();
        let w = s.frame.width as usize;
        s.frame.data[(y as usize * w + x as usize) * 4] = v;
    }

    pub fn fail_next(&self, e: ScreenError) {
        self.0.lock().unwrap().script.push_back(e);
    }

    pub fn data(&self) -> Vec<u8> {
        self.0.lock().unwrap().frame.data.clone()
    }

    pub fn gates(&self) -> u32 {
        self.0.lock().unwrap().gates
    }

    pub fn resize(&self, width: u32, height: u32) {
        self.0.lock().unwrap().frame = Frame {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        };
    }
}

pub struct Fake(pub Shared);

impl Screen for Fake {
    fn grab(&mut self, gate: &mut dyn FnMut()) -> Result<Frame, ScreenError> {
        let mut s = self.0 .0.lock().unwrap();
        if let Some(e) = s.script.pop_front() {
            return Err(e);
        }
        s.gates += 1;
        gate();
        Ok(s.frame.clone())
    }
}
