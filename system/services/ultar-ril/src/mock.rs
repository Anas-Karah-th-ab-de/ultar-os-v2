//! Test-Backend: ein `ModemLink`, der vorab eingereihte Antwort-Frames zurückgibt.
//! Nur für Tests ohne Gerät. In lib.rs mit `#[cfg(test)] mod mock;` einbinden.

use crate::link::{ModemLink, SipcFrame};
use std::collections::VecDeque;

pub struct MockLink {
    pub sent: Vec<(u8, Vec<u8>)>,
    pub inbox: VecDeque<SipcFrame>,
}

impl MockLink {
    pub fn new() -> Self {
        Self {
            sent: Vec::new(),
            inbox: VecDeque::new(),
        }
    }

    /// Mit genau einem Antwort-Frame vorbefüllen.
    pub fn with_frame(channel: u8, payload: Vec<u8>) -> Self {
        let mut m = Self::new();
        m.inbox.push_back(SipcFrame { channel, payload });
        m
    }

    pub fn push(&mut self, channel: u8, payload: Vec<u8>) {
        self.inbox.push_back(SipcFrame { channel, payload });
    }
}

impl Default for MockLink {
    fn default() -> Self {
        Self::new()
    }
}

impl ModemLink for MockLink {
    fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()> {
        self.sent.push((channel, payload.to_vec()));
        Ok(())
    }

    fn recv(&mut self, _timeout_ms: u32) -> std::io::Result<Option<SipcFrame>> {
        Ok(self.inbox.pop_front())
    }
}
