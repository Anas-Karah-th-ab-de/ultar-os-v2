//! GSP firmware lifecycle and bounded transport contract.

use super::{Error, Result};

pub const MAX_FIRMWARE_BYTES: usize = 64 << 20;
pub const DEFAULT_TIMEOUT_US: u32 = 500_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Empty,
    Prepared,
    Running,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Firmware<'a> {
    bytes: &'a [u8],
}

impl<'a> Firmware<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_FIRMWARE_BYTES {
            return Err(Error::InvalidArgument);
        }
        Ok(Self { bytes })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// Device-specific BAR/mailbox implementation supplied by the NVIDIA backend.
pub trait Transport {
    fn upload_firmware(&mut self, firmware: &[u8]) -> Result<()>;
    fn ring_doorbell(&mut self) -> Result<()>;
    fn ready(&mut self, timeout_us: u32) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}

pub struct Controller<'a> {
    firmware: Firmware<'a>,
    state: State,
}

impl<'a> Controller<'a> {
    pub fn prepare(firmware: Firmware<'a>) -> Self {
        Self {
            firmware,
            state: State::Prepared,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn firmware(&self) -> Firmware<'a> {
        self.firmware
    }

    pub fn start<T: Transport>(&mut self, transport: &mut T, timeout_us: u32) -> Result<()> {
        if self.state != State::Prepared {
            return Err(Error::InvalidState);
        }
        transport.upload_firmware(self.firmware.bytes())?;
        transport.ring_doorbell()?;
        transport.ready(timeout_us.max(1))?;
        self.state = State::Running;
        Ok(())
    }

    pub fn stop<T: Transport>(&mut self, transport: &mut T) -> Result<()> {
        if self.state != State::Running {
            return Err(Error::InvalidState);
        }
        transport.stop()?;
        self.state = State::Prepared;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock {
        calls: [u8; 4],
    }

    impl Transport for Mock {
        fn upload_firmware(&mut self, firmware: &[u8]) -> Result<()> {
            self.calls[0] = firmware.len() as u8;
            Ok(())
        }

        fn ring_doorbell(&mut self) -> Result<()> {
            self.calls[1] += 1;
            Ok(())
        }

        fn ready(&mut self, timeout_us: u32) -> Result<()> {
            self.calls[2] = (timeout_us != 0) as u8;
            Ok(())
        }

        fn stop(&mut self) -> Result<()> {
            self.calls[3] += 1;
            Ok(())
        }
    }

    #[test]
    fn lifecycle_is_bounded_and_ordered() {
        let firmware = Firmware::from_bytes(&[1, 2, 3]).unwrap();
        let mut controller = Controller::prepare(firmware);
        let mut mock = Mock { calls: [0; 4] };
        controller.start(&mut mock, DEFAULT_TIMEOUT_US).unwrap();
        assert_eq!(controller.state(), State::Running);
        assert_eq!(mock.calls, [3, 1, 1, 0]);
        controller.stop(&mut mock).unwrap();
        assert_eq!(mock.calls[3], 1);
    }
}
