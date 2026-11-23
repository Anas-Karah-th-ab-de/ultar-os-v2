//! Waiting for descriptors to become readable.

use crate::Errno;
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use std::os::fd::BorrowedFd;
use std::time::Duration;

/// Events that count as "readable": data, end of file, error, hang-up.
fn ready_flags() -> PollFlags {
    PollFlags::IN | PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL
}

/// Blocks until at least one of two descriptors is readable (or broken:
/// closed peer, error) and says which. Interrupted waits are repeated.
pub fn wait_readable(first: BorrowedFd<'_>, second: BorrowedFd<'_>) -> Result<(bool, bool), Errno> {
    let ready = PollFlags::IN | PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL;
    let mut fds = [
        PollFd::new(&first, PollFlags::IN),
        PollFd::new(&second, PollFlags::IN),
    ];
    loop {
        match poll(&mut fds, None) {
            Ok(_) => break,
            Err(rustix::io::Errno::INTR) => {}
            Err(errno) => return Err(errno.into()),
        }
    }
    Ok((
        fds[0].revents().intersects(ready),
        fds[1].revents().intersects(ready),
    ))
}

/// Waits until at least one of `fds` is readable (or broken) or `timeout`
/// passed (`None`: no limit) and returns one flag per descriptor.
/// Interrupted waits are repeated with the full timeout again (the caller
/// computes deadlines itself).
pub fn poll_readable(
    fds: &[BorrowedFd<'_>],
    timeout: Option<Duration>,
) -> Result<Vec<bool>, Errno> {
    let mut polled: Vec<PollFd<'_>> = fds
        .iter()
        .map(|fd| PollFd::new(fd, PollFlags::IN))
        .collect();
    let limit = timeout.map(|timeout| Timespec {
        tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(timeout.subsec_nanos()),
    });
    loop {
        match poll(&mut polled, limit.as_ref()) {
            Ok(_) => break,
            Err(rustix::io::Errno::INTR) => {}
            Err(errno) => return Err(errno.into()),
        }
    }
    Ok(polled
        .iter()
        .map(|fd| fd.revents().intersects(ready_flags()))
        .collect())
}
