//! The raw system calls: the **only** file of this crate with `unsafe`.
//!
//! Rules of this file:
//!
//! * every `unsafe` block has a `SAFETY` comment;
//! * every public function is safe: all pointers passed to the kernel point
//!   to local variables that outlive the call, all lengths are the sizes
//!   of those variables;
//! * nothing here allocates on the call path or panics.

use crate::Errno;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

#[cfg(target_arch = "x86_64")]
mod nr {
    pub const CLOSE: usize = 3;
    pub const RT_SIGPROCMASK: usize = 14;
    pub const GETSOCKOPT: usize = 55;
    pub const FCNTL: usize = 72;
    pub const PRCTL: usize = 157;
    pub const UNSHARE: usize = 272;
    pub const SIGNALFD4: usize = 289;
}

#[cfg(target_arch = "aarch64")]
mod nr {
    pub const FCNTL: usize = 25;
    pub const CLOSE: usize = 57;
    pub const SIGNALFD4: usize = 74;
    pub const RT_SIGPROCMASK: usize = 135;
    pub const UNSHARE: usize = 97;
    pub const PRCTL: usize = 167;
    pub const GETSOCKOPT: usize = 209;
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("ultar-sys supports x86_64 and aarch64 only");

/// Issues one system call with up to five arguments.
///
/// # Safety
///
/// The caller guarantees that the arguments are valid for system call `nr`:
/// every pointer argument points to memory that is valid for the access the
/// kernel performs and stays valid until the call returns.
// SAFETY: an `unsafe fn`; its contract is the `# Safety` section above.
#[cfg(target_arch = "x86_64")]
unsafe fn raw(nr: usize, a: [usize; 5]) -> isize {
    let ret: isize;
    // SAFETY: the `syscall` instruction with the Linux x86_64 register
    // convention; it clobbers only rax, rcx and r11, which are declared.
    // Validity of the arguments is the caller's obligation (see above).
    unsafe {
        std::arch::asm!(
            "syscall",
            inlateout("rax") nr as isize => ret,
            in("rdi") a[0],
            in("rsi") a[1],
            in("rdx") a[2],
            in("r10") a[3],
            in("r8") a[4],
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issues one system call with up to five arguments.
///
/// # Safety
///
/// See the x86_64 variant.
// SAFETY: an `unsafe fn`; its contract is the `# Safety` section above.
#[cfg(target_arch = "aarch64")]
unsafe fn raw(nr: usize, a: [usize; 5]) -> isize {
    let ret: isize;
    // SAFETY: the `svc 0` instruction with the Linux aarch64 register
    // convention (number in x8, arguments in x0..x4, result in x0).
    unsafe {
        std::arch::asm!(
            "svc 0",
            in("x8") nr,
            inlateout("x0") a[0] as isize => ret,
            in("x1") a[1],
            in("x2") a[2],
            in("x3") a[3],
            in("x4") a[4],
            options(nostack),
        );
    }
    ret
}

fn check(ret: isize) -> Result<usize, Errno> {
    if (-4095..0).contains(&ret) {
        Err(Errno(i32::try_from(-ret).unwrap_or(i32::MAX)))
    } else {
        Ok(ret as usize)
    }
}

/// Size of the kernel's signal set in bytes.
const SIGSET_BYTES: usize = 8;
/// `SIG_BLOCK`.
const SIG_BLOCK: usize = 0;
/// `SFD_CLOEXEC`.
const SFD_CLOEXEC: usize = 0o2_000_000;

/// Adds the signals in `mask` (bit `n - 1` for signal `n`) to the blocked
/// signals of the calling thread and returns the previous mask. Threads
/// created afterwards inherit the mask.
pub fn block_signals(mask: u64) -> Result<u64, Errno> {
    let mut old: u64 = 0;
    // SAFETY: both pointers refer to local `u64` values (8 bytes each, the
    // size passed as fourth argument) that live across the call.
    let ret = unsafe {
        raw(
            nr::RT_SIGPROCMASK,
            [
                SIG_BLOCK,
                std::ptr::from_ref(&mask) as usize,
                std::ptr::from_mut(&mut old) as usize,
                SIGSET_BYTES,
                0,
            ],
        )
    };
    check(ret).map(|_| old)
}

/// `SIG_SETMASK`.
const SIG_SETMASK: usize = 2;

/// Replaces the blocked signals of the calling thread with `mask` and
/// returns the previous mask. A launcher calls it with 0 before `execve`:
/// the blocked mask is inherited across `execve`, and a service must not
/// start with the supervisor's signals blocked.
pub fn set_signal_mask(mask: u64) -> Result<u64, Errno> {
    let mut old: u64 = 0;
    // SAFETY: both pointers refer to local `u64` values (8 bytes each, the
    // size passed as fourth argument) that live across the call.
    let ret = unsafe {
        raw(
            nr::RT_SIGPROCMASK,
            [
                SIG_SETMASK,
                std::ptr::from_ref(&mask) as usize,
                std::ptr::from_mut(&mut old) as usize,
                SIGSET_BYTES,
                0,
            ],
        )
    };
    check(ret).map(|_| old)
}

/// Creates a descriptor from which the signals in `mask` can be read as
/// 128-byte `signalfd_siginfo` records (signal number in the first four
/// bytes). The signals must be blocked, see [`block_signals`].
pub fn signal_fd(mask: u64) -> Result<OwnedFd, Errno> {
    // SAFETY: the mask pointer refers to a local `u64` of the stated size.
    let ret = unsafe {
        raw(
            nr::SIGNALFD4,
            [
                usize::MAX,
                std::ptr::from_ref(&mask) as usize,
                SIGSET_BYTES,
                SFD_CLOEXEC,
                0,
            ],
        )
    };
    let fd = i32::try_from(check(ret)?).map_err(|_| Errno::EBADF)?;
    // SAFETY: the kernel just returned `fd` as a new descriptor that nothing
    // else in this process knows, so taking ownership is sound.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Credentials of the process on the other end of a Unix socket.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ucred {
    /// Process id.
    pub pid: u32,
    /// Effective user id at `connect` time.
    pub uid: u32,
    /// Effective group id at `connect` time.
    pub gid: u32,
}

/// `SOL_SOCKET`.
const SOL_SOCKET: usize = 1;
/// `SO_PEERCRED`.
const SO_PEERCRED: usize = 17;

/// Reads `SO_PEERCRED` of a connected Unix socket.
///
/// A kernel that does not implement the option is reported as
/// [`Errno::ENOPROTOOPT`]: that is the case when the call fails, when the
/// kernel returns fewer than the twelve bytes of `struct ucred`, or when it
/// reports process id 0 (no process has that id, so the answer cannot be a
/// real credential). The Ultar kernel at the time of writing answers every
/// unknown socket option with four zero bytes and is caught by this check.
pub fn peer_credentials(socket: BorrowedFd<'_>) -> Result<Ucred, Errno> {
    let mut cred = [0u32; 3];
    let mut len: u32 = 12;
    // SAFETY: `cred` is 12 bytes, `len` says so and is a valid in/out
    // pointer; the descriptor is borrowed and therefore open.
    let ret = unsafe {
        raw(
            nr::GETSOCKOPT,
            [
                socket.as_raw_fd() as usize,
                SOL_SOCKET,
                SO_PEERCRED,
                std::ptr::from_mut(&mut cred) as usize,
                std::ptr::from_mut(&mut len) as usize,
            ],
        )
    };
    check(ret)?;
    if len != 12 || cred[0] == 0 {
        return Err(Errno::ENOPROTOOPT);
    }
    Ok(Ucred {
        pid: cred[0],
        uid: cred[1],
        gid: cred[2],
    })
}

/// `F_GETFD`.
const F_GETFD: usize = 1;
/// `F_SETFD`.
const F_SETFD: usize = 2;
/// `FD_CLOEXEC`.
const FD_CLOEXEC: usize = 1;

/// Whether descriptor number `fd` is open in this process.
pub fn is_open(fd: i32) -> bool {
    let Ok(number) = usize::try_from(fd) else {
        return false;
    };
    // SAFETY: `fcntl(F_GETFD)` only inspects the descriptor table.
    check(unsafe { raw(nr::FCNTL, [number, F_GETFD, 0, 0, 0]) }).is_ok()
}

/// Takes ownership of the inherited descriptor `fd` and marks it
/// close-on-exec.
///
/// Only [`crate::inherit::take_inherited`] calls this – once per process,
/// before the process has opened anything itself – so the number can only
/// name a descriptor the parent passed on, and nothing else owns it.
pub(crate) fn adopt_number(fd: i32) -> Result<OwnedFd, Errno> {
    if fd < 3 || !is_open(fd) {
        return Err(Errno::EBADF);
    }
    // SAFETY: only integer arguments; marks the descriptor close-on-exec.
    check(unsafe { raw(nr::FCNTL, [fd as usize, F_SETFD, FD_CLOEXEC, 0, 0]) })?;
    // SAFETY: `fd` is open (checked above); by this function's contract
    // (one call per number, before anything owns a descriptor above the
    // standard streams) no other owner exists.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Closes the inherited descriptor `fd`; `true` if it was open.
///
/// Same contract as [`adopt_number`]: only called by
/// [`crate::inherit::take_inherited`] before anything in this process owns
/// a descriptor above the standard streams.
pub(crate) fn close_number(fd: i32) -> bool {
    if fd < 3 || !is_open(fd) {
        return false;
    }
    // SAFETY: by the contract above no Rust object owns `fd`; it is a
    // descriptor leaked into this process by its parent.
    check(unsafe { raw(nr::CLOSE, [fd as usize, 0, 0, 0, 0]) }).is_ok()
}

/// `prctl` option of the Ultar kernel: read the rights of the process.
const PR_ULTAR_RIGHTS_GET: usize = 0x554c_5200;
/// `prctl` option of the Ultar kernel: drop the rights in the mask.
const PR_ULTAR_RIGHTS_DROP: usize = 0x554c_5201;

/// The rights of this process on the Ultar kernel, as its bit mask. Any
/// other kernel does not know the option and answers with an error.
pub fn ultar_rights() -> Result<u64, Errno> {
    // SAFETY: the option takes no argument and only returns an integer.
    check(unsafe { raw(nr::PRCTL, [PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0]) }).map(|bits| bits as u64)
}

/// Drops the Ultar kernel rights in `mask` for this process (rights can
/// only shrink).
pub fn ultar_drop_rights(mask: u64) -> Result<(), Errno> {
    // SAFETY: the option takes the mask as a plain integer, no pointer.
    check(unsafe { raw(nr::PRCTL, [PR_ULTAR_RIGHTS_DROP, mask as usize, 0, 0, 0]) }).map(|_| ())
}

/// `prctl` option of the Ultar kernel: set the "internet only" barrier (one way, inherited).
const PR_ULTAR_NET_GUARD: usize = 0x554c_5202;
/// `prctl` option of the Ultar kernel: whether the barrier is set.
const PR_ULTAR_NET_GUARD_GET: usize = 0x554c_5203;

/// Sets the Ultar kernel's "internet only" barrier on this process and
/// every later child: no host loopback, no address of the device, no LAN,
/// broadcast or multicast; only the internet and the leased DNS servers on
/// port 53. There is no way back. Other kernels answer with an error.
pub fn ultar_net_guard() -> Result<(), Errno> {
    // SAFETY: the option takes no argument and only returns an integer.
    check(unsafe { raw(nr::PRCTL, [PR_ULTAR_NET_GUARD, 0, 0, 0, 0]) }).map(|_| ())
}

/// Whether this process carries the "internet only" barrier.
pub fn ultar_net_guarded() -> Result<bool, Errno> {
    // SAFETY: the option takes no argument and only returns an integer.
    check(unsafe { raw(nr::PRCTL, [PR_ULTAR_NET_GUARD_GET, 0, 0, 0, 0]) }).map(|v| v == 1)
}

/// `CLONE_NEWNS | CLONE_NEWUTS | CLONE_NEWIPC | CLONE_NEWPID | CLONE_NEWNET`:
/// the only flags [`unshare_namespaces`] passes on. Notably not
/// `CLONE_FILES`, `CLONE_FS` or `CLONE_NEWUSER`.
const NAMESPACE_FLAGS: usize = 0x0002_0000 | 0x0400_0000 | 0x0800_0000 | 0x2000_0000 | 0x4000_0000;

/// `unshare(flags)` restricted to the namespace flags of
/// [`NAMESPACE_FLAGS`]; any other bit is refused with `EINVAL` before the
/// kernel is asked. (`rustix` marks its `unshare` unsafe because of
/// `CLONE_FILES`, which this wrapper never passes.)
pub fn unshare_namespaces(flags: usize) -> Result<(), Errno> {
    if flags & !NAMESPACE_FLAGS != 0 || flags == 0 {
        return Err(Errno::EINVAL);
    }
    // SAFETY: a single integer argument; only namespace flags (checked
    // above), which change what the calling thread sees, never memory or
    // the descriptor table of this process.
    check(unsafe { raw(nr::UNSHARE, [flags, 0, 0, 0, 0]) }).map(|_| ())
}
