//! Servo wakes the host's loop from its own threads by writing one byte into a pipe; the loop
//! waits on the pipe together with the browser socket.

use rustix::pipe::{pipe_with, PipeFlags};
use std::io::{self, Read};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::Arc;

/// The reading end, owned by the loop.
pub struct Wakeups {
    read: std::fs::File,
    write: Arc<OwnedFd>,
}

impl Wakeups {
    /// A new pipe (both ends non-blocking, closed on exec).
    pub fn new() -> io::Result<Wakeups> {
        let (read, write) = pipe_with(PipeFlags::NONBLOCK | PipeFlags::CLOEXEC)?;
        Ok(Wakeups {
            read: std::fs::File::from(read),
            write: Arc::new(write),
        })
    }

    /// What Servo calls from any thread.
    pub fn waker(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(Waker(self.write.clone()))
    }

    /// The descriptor to wait on.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.read.as_fd()
    }

    /// Empties the pipe after a wake-up.
    pub fn drain(&mut self) {
        let mut sink = [0u8; 256];
        while matches!(self.read.read(&mut sink), Ok(n) if n > 0) {}
    }
}

struct Waker(Arc<OwnedFd>);

impl servo::EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(Waker(self.0.clone()))
    }

    fn wake(&self) {
        // a full pipe already means "wake up": a failed write loses nothing
        let _ = rustix::io::write(&*self.0, &[1]);
    }
}
