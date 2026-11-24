//! Validated GPU command submission API.

use super::{Error, Result};
use alloc::vec::Vec;

pub const MAX_COMMAND_WORDS: usize = 1 << 20;

pub struct CommandBuffer {
    words: Vec<u32>,
    sealed: bool,
}

impl CommandBuffer {
    pub fn new() -> Self {
        Self {
            words: Vec::new(),
            sealed: false,
        }
    }

    pub fn push(&mut self, word: u32) -> Result<()> {
        if self.sealed || self.words.len() >= MAX_COMMAND_WORDS {
            return Err(Error::InvalidState);
        }
        self.words.push(word);
        Ok(())
    }

    pub fn seal(&mut self) -> Result<()> {
        if self.words.is_empty() {
            return Err(Error::InvalidArgument);
        }
        self.sealed = true;
        Ok(())
    }

    pub fn words(&self) -> &[u32] {
        &self.words
    }
}

pub trait Backend {
    fn submit(&mut self, commands: &[u32]) -> Result<u64>;
}

pub struct Engine<B> {
    backend: B,
}

impl<B: Backend> Engine<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub fn submit(&mut self, commands: &CommandBuffer) -> Result<u64> {
        if !commands.sealed {
            return Err(Error::InvalidState);
        }
        self.backend.submit(commands.words())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock;

    impl Backend for Mock {
        fn submit(&mut self, commands: &[u32]) -> Result<u64> {
            Ok(commands.len() as u64)
        }
    }

    #[test]
    fn commands_must_be_sealed() {
        let mut commands = CommandBuffer::new();
        commands.push(0xdead_beef).unwrap();
        let mut engine = Engine::new(Mock);
        assert!(engine.submit(&commands).is_err());
        commands.seal().unwrap();
        assert_eq!(engine.submit(&commands).unwrap(), 1);
        assert!(commands.push(1).is_err());
    }
}
