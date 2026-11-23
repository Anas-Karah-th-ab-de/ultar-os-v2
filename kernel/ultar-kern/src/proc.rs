//! Prozesse des Ultar-Kerns: eigener Adressraum, ELF-Lader, Planer (zeitgesteuert, reihum),
//! Warten (Eingabe, Kindprozesse, Rohre, Schlaf) – Systemaufrufe in syscall.rs.
//!
//! Programme laufen in Ring 3. Systemaufrufe folgen den Linux-Nummern (x86_64), damit unveränderte statische
//! Linux-Programme – etwa `busybox` aus Ubuntu – auf dem Ultar-Kern laufen.

use crate::fs;
use crate::paging::{AddrSpace, USER_LOW, USER_LOW_END};
use crate::trap::{ultar_back, ultar_resume, TrapFrame};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

pub const KSTACK: usize = 32 * 1024;
pub const STACK_TOP: u64 = 0x7fff_ffff_f000;
pub const STACK_MAX: u64 = 8 << 20;
pub const MMAP_BASE: u64 = 0x7f00_0000_0000;
pub const PIE_BASE: u64 = 0x5555_5555_4000;

pub use crate::pipe::Pipe;

/// Highest descriptor number + 1 of one process (the table is a vector indexed by number).
pub const MAX_FDS: usize = 65536;

pub enum FdKind {
    Console,
    Null,
    Zero,
    Random,
    File { data: Arc<Vec<u8>>, pos: u64, path: String, append: bool, write: bool, dirty: bool },
    /// `path` ist der Pfad im Dateibaum des Kerns; `view`/`tag`: path as the opening process saw it and the
    /// view it was opened in (base for *at calls, see isolation::path)
    /// (the listing is shared between duplicates of the descriptor – a directory can be large)
    Dir { ents: Arc<Vec<fs::DirEnt>>, pos: usize, path: String, view: String, tag: crate::isolation::ViewTag },
    PipeR(Arc<Pipe>),
    PipeW(Arc<Pipe>),
    Sock(Arc<crate::socket::Handle>),
    /// signalfd: Maske der gelesenen Signale
    SigFd(u64),
    /// verbundener Unix-Socket: liest aus rx, schreibt in tx
    UnixConn(Arc<Pipe>, Arc<Pipe>),
    /// Unix-Socket ohne Verbindung (ungebunden oder lauschend)
    UnixSock(Arc<Mutex<UnixState>>),
    /// epoll: beobachtete (fd, Ereignisse, Daten)
    Epoll(Arc<Mutex<Vec<(i32, u32, u64)>>>),
    /// eventfd: Zähler, Semaphor-Modus
    EventFd(Arc<Mutex<u64>>, bool),
    /// timerfd: (nächster Ablauf in ms seit Start, Intervall in ms; 0 = aus)
    TimerFd(Arc<Mutex<(u64, u64)>>),
    /// Bildspeicher /dev/fb0
    Fb(Arc<crate::eingabe::FbFd>),
    /// Eingabegerät /dev/input/eventN (Maus/Tastatur)
    Eingabe(Arc<crate::eingabe::EingabeFd>),
    /// Namespace handle (`/proc/<pid>/ns/*`), argument of `setns`
    Ns(crate::isolation::NsRef),
}

pub enum UnixState {
    Unbound,
    Listening(String, alloc::collections::VecDeque<(Arc<Pipe>, Arc<Pipe>)>),
}

/// Unix-Verbindung als Paar von Rohren; Zähler wie bei offenen Enden
pub fn unix_pair() -> (FdKind, FdKind) {
    let (a, b) = (Pipe::with_capacity(crate::pipe::UNIX_DEFAULT), Pipe::with_capacity(crate::pipe::UNIX_DEFAULT));
    for p in [&a, &b] {
        p.readers.store(1, Ordering::SeqCst);
        p.writers.store(1, Ordering::SeqCst);
    }
    (FdKind::UnixConn(a.clone(), b.clone()), FdKind::UnixConn(b, a))
}

pub struct Fd {
    pub kind: FdKind,
    pub cloexec: bool,
    pub flags: u32,
}

impl Fd {
    pub fn dup(&self) -> Fd {
        let kind = match &self.kind {
            FdKind::Console => FdKind::Console,
            FdKind::Null => FdKind::Null,
            FdKind::Zero => FdKind::Zero,
            FdKind::Random => FdKind::Random,
            FdKind::File { data, pos, path, append, write, dirty } => FdKind::File { data: data.clone(), pos: *pos, path: path.clone(), append: *append, write: *write, dirty: *dirty },
            FdKind::Dir { ents, pos, path, view, tag } => FdKind::Dir { ents: ents.clone(), pos: *pos, path: path.clone(), view: view.clone(), tag: *tag },
            FdKind::PipeR(p) => {
                p.readers.fetch_add(1, Ordering::SeqCst);
                FdKind::PipeR(p.clone())
            }
            FdKind::PipeW(p) => {
                p.writers.fetch_add(1, Ordering::SeqCst);
                FdKind::PipeW(p.clone())
            }
            FdKind::Sock(h) => FdKind::Sock(h.clone()),
            FdKind::SigFd(m) => FdKind::SigFd(*m),
            FdKind::UnixConn(r, w) => {
                r.readers.fetch_add(1, Ordering::SeqCst);
                w.writers.fetch_add(1, Ordering::SeqCst);
                FdKind::UnixConn(r.clone(), w.clone())
            }
            FdKind::UnixSock(s) => FdKind::UnixSock(s.clone()),
            FdKind::Epoll(e) => FdKind::Epoll(e.clone()),
            FdKind::EventFd(c, sem) => FdKind::EventFd(c.clone(), *sem),
            FdKind::TimerFd(t) => FdKind::TimerFd(t.clone()),
            FdKind::Fb(f) => FdKind::Fb(f.clone()),
            FdKind::Eingabe(e) => FdKind::Eingabe(e.clone()),
            FdKind::Ns(n) => FdKind::Ns(n.clone()),
        };
        Fd { kind, cloexec: false, flags: self.flags }
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        match &self.kind {
            FdKind::PipeR(p) => {
                p.readers.fetch_sub(1, Ordering::SeqCst);
                crate::socket::bump();
            }
            FdKind::PipeW(p) => {
                p.writers.fetch_sub(1, Ordering::SeqCst);
                crate::socket::bump();
            }
            FdKind::UnixConn(r, w) => {
                r.readers.fetch_sub(1, Ordering::SeqCst);
                w.writers.fetch_sub(1, Ordering::SeqCst);
                crate::socket::bump();
            }
            FdKind::UnixSock(s) => {
                if Arc::strong_count(s) == 1 {
                    crate::syscall::unix_unlisten(s);
                }
            }
            FdKind::File { data, path, write: true, dirty: true, .. } => {
                // the buffer itself becomes the stored file (no copy)
                fs::store_file(path, data.clone());
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wait {
    Input,
    Child(i64),
    Pipe,
    Sleep(u64),
    Vfork(u64),
    /// Futex: physische Adresse, Frist in ms (0 = keine)
    Futex(u64, u64),
    /// Netz: auf ein Netzereignis warten (Stand des Ereigniszählers beim Warten)
    Net(u64),
    /// sigsuspend/pause: bis ein Signal kommt
    Signal,
    /// poll/select: Ereigniszähler und Frist (0 = keine)
    Poll(u64, u64),
}

/// Gemeinsam genutzter Zustand (Fäden einer Gruppe teilen ihn). Zugriff nur unter der Sperre `PROCS`.
pub struct Shared<T>(Arc<core::cell::UnsafeCell<T>>);
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}
impl<T> Shared<T> {
    pub fn new(t: T) -> Shared<T> {
        Shared(Arc::new(core::cell::UnsafeCell::new(t)))
    }
    pub fn refs(&self) -> usize {
        Arc::strong_count(&self.0)
    }
    pub fn same(&self, o: &Shared<T>) -> bool {
        Arc::ptr_eq(&self.0, &o.0)
    }
}
impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}
impl<T> core::ops::Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.0.get() }
    }
}
impl<T> core::ops::DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

#[derive(Clone, Copy, Default)]
pub struct SigAct {
    pub handler: u64,
    pub flags: u64,
    pub restorer: u64,
    pub mask: u64,
}

/// Zustand einer Fadengruppe (= ein Linux-„Prozess“)
pub struct Group {
    pub space: Option<AddrSpace>,
    pub fds: Vec<Option<Fd>>,
    pub cwd: String,
    pub brk_start: u64,
    pub brk: u64,
    pub brk_limit: u64,
    pub mmap_next: u64,
    pub umask: u32,
    pub exe: String,
    pub cmdline: Vec<String>,
    pub sigact: [SigAct; 65],
    /// Hilfsvektor (auxv) wie beim Start auf den Stapel gelegt – für /proc/<pid>/auxv
    pub auxv: Vec<u8>,
    /// Isolation: mount namespace, root, pid namespace of children, rights (shared by the threads)
    pub iso: crate::isolation::Isolation,
    /// Host path of the running program (`exe` is the path the process itself sees)
    pub exe_host: String,
    /// Resource group (see groups.rs) the threads of this process belong to
    pub res: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Ready,
    Blocked(Wait),
    /// beendet, wartet auf wait4 des Elternprozesses
    Zombie(i32),
    /// Gruppenführer beendet, andere Fäden laufen noch
    Exiting,
    /// Faden beendet, wird entfernt
    Dead,
    /// läuft gerade auf Kern n
    Running(usize),
}

pub struct Proc {
    /// Faden-Kennung (tid)
    pub pid: u64,
    /// Gruppe (getpid)
    pub tgid: u64,
    pub ppid: u64,
    pub pgid: u64,
    pub name: String,
    pub state: State,
    pub frame: Box<TrapFrame>,
    pub kstack: Vec<u8>,
    pub fs_base: u64,
    pub clear_tid: u64,
    pub sigmask: u64,
    pub pending: u64,
    /// Maske vor sigsuspend (wird nach dem Handler wiederhergestellt)
    pub sus_mask: Option<u64>,
    /// von einem anderen Kern beendet, während der Faden lief (Status)
    pub killed: Option<i32>,
    /// Kernstapel noch in Benutzung (erst danach freigeben)
    pub on_cpu: bool,
    /// Frist eines wiederholten poll/select/epoll_wait: (Aufrufstelle, Frist in ms)
    pub poll_dl: Option<(u64, u64)>,
    pub g: Shared<Group>,
    /// Membership in the pid namespace (numbers of this thread in its namespace and all ancestors)
    pub link: crate::isolation::pid::PidLink,
    /// This thread's count in its resource group (released when the thread is reaped)
    pub pids: crate::groups::PidCharge,
    /// Bytes a blocking `write` to a full pipe has delivered so far (the call continues later)
    pub io_done: u64,
}

impl Proc {
    #[track_caller]
    pub fn space(&self) -> &AddrSpace {
        self.g.space.as_ref().unwrap()
    }
    #[track_caller]
    pub fn space_mut(&mut self) -> &mut AddrSpace {
        self.g.space.as_mut().unwrap()
    }
    pub fn fd(&mut self, n: u64) -> Option<&mut Fd> {
        // Deskriptoren sind `int`: obere Bits des Registers zählen nicht (glibc lässt sie stehen)
        self.g.fds.get_mut(n as u32 as usize)?.as_mut()
    }
    /// Put `fd` at the lowest free number from `min` on. Errors: `-EMFILE` (24) beyond
    /// [`MAX_FDS`], `-ENOMEM` (12) if the table cannot grow.
    pub fn add_fd(&mut self, fd: Fd, min: usize) -> i64 {
        let fds = &mut self.g.fds;
        let mut i = min;
        loop {
            if i >= MAX_FDS {
                return -24;
            }
            if i >= fds.len() {
                if !crate::mem::try_reserve(fds, i + 1 - fds.len()) {
                    return -12;
                }
                fds.resize_with(i + 1, || None);
            }
            if fds[i].is_none() {
                fds[i] = Some(fd);
                return i as i64;
            }
            i += 1;
        }
    }
}

pub static PROCS: Mutex<Vec<Box<Proc>>> = Mutex::new(Vec::new());
/// Arbeitsverzeichnis der Kern-Konsole (`cd`); neue Programme starten dort. Leer = "/".
pub static SHELL_CWD: Mutex<String> = Mutex::new(String::new());
static NEXT_PID: AtomicU64 = AtomicU64::new(1);
static WINDOW_OK: AtomicBool = AtomicBool::new(true);
pub static INTERRUPT: AtomicBool = AtomicBool::new(false);
static LAST_INT: AtomicU64 = AtomicU64::new(0);

pub fn current() -> u64 {
    crate::percpu::me().current
}

fn sched_ctx() -> *mut u64 {
    core::ptr::addr_of_mut!(crate::percpu::me().sched_ctx)
}

/// Prüfen, ob der Kern selbst im Fenster für fest gebundene Programme liegt (dann nur PIE-Programme)
pub fn check_window() {
    let rsp: u64;
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp) };
    let code = check_window as usize as u64;
    let bad = |a: u64| (USER_LOW..USER_LOW_END).contains(&a);
    if bad(rsp) || bad(code) {
        WINDOW_OK.store(false, Ordering::Relaxed);
        crate::kprintln!("[proc] Kern liegt bei {code:#x} – fest gebundene Programme (ET_EXEC) abgeschaltet");
    }
}

// ------------------------------------------------------------------ Benutzerspeicher

pub fn read_user(sp: &AddrSpace, va: u64, len: usize) -> Option<Vec<u8>> {
    let mut out = crate::mem::try_with_capacity(len)?;
    let mut a = va;
    while out.len() < len {
        let (pa, _) = sp.translate(a)?;
        let n = (4096 - (a & 0xfff) as usize).min(len - out.len());
        out.extend_from_slice(unsafe { core::slice::from_raw_parts(pa as *const u8, n) });
        a += n as u64;
    }
    Some(out)
}

pub fn write_user(sp: &AddrSpace, va: u64, data: &[u8]) -> bool {
    let mut off = 0;
    while off < data.len() {
        let a = va + off as u64;
        let Some((pa, w)) = sp.translate(a) else { return false };
        if !w {
            return false;
        }
        let n = (4096 - (a & 0xfff) as usize).min(data.len() - off);
        unsafe { core::ptr::copy_nonoverlapping(data[off..].as_ptr(), pa as *mut u8, n) };
        off += n;
    }
    true
}

pub fn read_cstr(sp: &AddrSpace, va: u64) -> Option<String> {
    let mut v = Vec::new();
    let mut a = va;
    loop {
        let (pa, _) = sp.translate(a)?;
        let b = unsafe { *(pa as *const u8) };
        if b == 0 {
            break;
        }
        v.push(b);
        a += 1;
        if v.len() > 4096 {
            return None;
        }
    }
    Some(String::from_utf8_lossy(&v).into_owned())
}

pub fn read_u64(sp: &AddrSpace, va: u64) -> Option<u64> {
    read_user(sp, va, 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}

/// Most bytes of an argument or environment vector (the start-up stack holds 200 KiB in all).
pub const MAX_ARG_BYTES: usize = 192 * 1024;

pub fn read_strv(sp: &AddrSpace, mut va: u64) -> Option<Vec<String>> {
    let mut v = Vec::new();
    if va == 0 {
        return Some(v);
    }
    let mut bytes = 0;
    loop {
        let p = read_u64(sp, va)?;
        if p == 0 {
            return Some(v);
        }
        let s = read_cstr(sp, p)?;
        bytes += s.len() + 9;
        v.push(s);
        va += 8;
        // too long for the stack anyway: stop before the copy grows without need
        if v.len() > 4096 || bytes > MAX_ARG_BYTES {
            crate::percpu::me().errno = 7; // E2BIG
            return None;
        }
    }
}

/// Text of the error when a program cannot be loaded for lack of memory.
pub const NO_MEMORY: &str = "kein Speicher";

fn copy_to(sp: &mut AddrSpace, va: u64, data: &[u8], writable: bool, exec: bool) -> Result<(), &'static str> {
    let mut off = 0usize;
    let mut a = va;
    let end = va + data.len() as u64;
    while a < end || (off == 0 && data.is_empty()) {
        let page = a & !0xfff;
        let pa = sp.map(page, writable, exec).ok_or(NO_MEMORY)?;
        let n = ((page + 4096).min(end) - a) as usize;
        unsafe { core::ptr::copy_nonoverlapping(data[off..].as_ptr(), (pa + (a - page)) as *mut u8, n) };
        off += n;
        a += n as u64;
        if data.is_empty() {
            break;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ ELF

pub struct Image {
    pub auxv: Vec<u8>,
    pub space: AddrSpace,
    pub entry: u64,
    pub sp: u64,
    pub brk: u64,
    pub brk_limit: u64,
}

fn u16le(b: &[u8], o: usize) -> u64 {
    u16::from_le_bytes([b[o], b[o + 1]]) as u64
}
fn u32le(b: &[u8], o: usize) -> u64 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as u64
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

pub const INTERP_BASE: u64 = 0x7ff0_0000_0000;

/// Segmente eines ELF an `base` laden; liefert (Ende, Programmkopf-Adresse)
fn load_segments(sp: &mut AddrSpace, data: &[u8], base: u64) -> Result<(u64, u64), &'static str> {
    let phoff = u64le(data, 32) as usize;
    let phent = u16le(data, 54) as usize;
    let phnum = u16le(data, 56) as usize;
    if phent < 56 || phoff + phent * phnum > data.len() {
        return Err("ELF beschädigt");
    }
    let mut top = 0u64;
    let mut phdr = 0u64;
    for i in 0..phnum {
        let p = &data[phoff + i * phent..phoff + (i + 1) * phent];
        let pt = u32le(p, 0);
        if pt == 6 {
            phdr = base + u64le(p, 16);
        }
        if pt != 1 {
            continue;
        }
        let flags = u32le(p, 4);
        let off = u64le(p, 8) as usize;
        let va = base.wrapping_add(u64le(p, 16));
        let filesz = u64le(p, 32) as usize;
        let memsz = u64le(p, 40);
        if base == 0 && (va < USER_LOW || va + memsz > USER_LOW_END) {
            return Err("Programmadresse außerhalb des Benutzerfensters");
        }
        // addresses and sizes come from the file: a relocatable program must stay in the upper user
        // area (below it lies the kernel's identity map, which a user page would replace in this
        // address space), and a segment the user half cannot hold is refused
        let wrapped = base.checked_add(u64le(p, 16)) != Some(va);
        let outside = wrapped || memsz > 1 << 34 || va + memsz > STACK_TOP || base != 0 && va < 0x1000_0000_0000;
        if outside || off + filesz > data.len() {
            return Err("ELF beschädigt");
        }
        let (w, x) = (flags & 2 != 0, flags & 1 != 0);
        copy_to(sp, va, &data[off..off + filesz], w, x)?;
        let mut a = (va + filesz as u64 + 4095) & !4095;
        while a < va + memsz {
            sp.map(a, w, x).ok_or(NO_MEMORY)?;
            a += 4096;
        }
        if phdr == 0 && off == 0 {
            phdr = va + phoff as u64;
        }
        top = top.max(va + memsz);
    }
    if top == 0 {
        return Err("keine ladbaren Abschnitte");
    }
    Ok((top, phdr))
}

/// ELF64-Programm laden (statisch: ET_EXEC im Fenster 4 MB–256 MB oder PIE; dynamisch: mit ld.so aus dem
/// Dateibaum) und Stapel mit argv, envp, auxv anlegen
///
/// `read`: reads a file by the path the program names (the dynamic loader); the caller resolves it in the
/// view of the process that will run the program.
pub fn load_elf(data: &[u8], argv: &[String], envp: &[String], path: &str, read: &dyn Fn(&str) -> Option<Arc<Vec<u8>>>) -> Result<Image, &'static str> {
    if data.len() < 64 || &data[0..4] != b"\x7fELF" || data[4] != 2 || data[5] != 1 || u16le(data, 18) != 62 {
        return Err("kein x86_64-ELF-Programm");
    }
    let typ = u16le(data, 16);
    let phoff = u64le(data, 32) as usize;
    let phent = u16le(data, 54) as usize;
    let phnum = u16le(data, 56) as usize;
    if phent < 56 || phoff + phent * phnum > data.len() {
        return Err("ELF beschädigt");
    }
    let ph = |i: usize| &data[phoff + i * phent..phoff + (i + 1) * phent];
    let interp = (0..phnum).find(|&i| u32le(ph(i), 0) == 3).map(|i| {
        let p = ph(i);
        let (o, n) = (u64le(p, 8) as usize, u64le(p, 32) as usize);
        String::from_utf8_lossy(&data[o.min(data.len())..(o + n).min(data.len())]).trim_end_matches('\0').to_string()
    });
    let base = match typ {
        2 => {
            if !WINDOW_OK.load(Ordering::Relaxed) {
                return Err("fest gebundenes Programm nicht möglich (Kern im Fenster)");
            }
            0
        }
        3 => PIE_BASE,
        _ => return Err("ELF-Typ nicht unterstützt"),
    };
    let mut sp = AddrSpace::new().ok_or(NO_MEMORY)?;
    let (top, phdr) = load_segments(&mut sp, data, base)?;
    let mut auxv: Vec<u8> = Vec::new();
    let prog_entry = base + u64le(data, 24);
    let (entry, interp_base) = match interp {
        Some(ip) => {
            let idata = read(&ip).ok_or("Programmlader (ld.so) nicht gefunden")?;
            if idata.len() < 64 || &idata[0..4] != b"\x7fELF" || u16le(&idata, 16) != 3 {
                return Err("Programmlader ist kein ELF");
            }
            load_segments(&mut sp, &idata, INTERP_BASE)?;
            (INTERP_BASE + u64le(&idata, 24), INTERP_BASE)
        }
        None => (prog_entry, 0),
    };
    let brk = (top + 4095) & !4095;
    let brk_limit = if base == 0 { USER_LOW_END } else { brk + (1 << 30) };

    // Stapel: 256 KB sofort, Rest wächst bei Bedarf
    let mut a = STACK_TOP - 256 * 1024;
    while a < STACK_TOP {
        sp.map(a, true, false).ok_or(NO_MEMORY)?;
        a += 4096;
    }
    let total: usize = argv.iter().chain(envp.iter()).map(|s| s.len() + 1).sum::<usize>() + path.len() + 64;
    let mut strs: Vec<u8> = crate::mem::try_with_capacity(total).ok_or(NO_MEMORY)?;
    let mut put = |s: &[u8]| -> u64 {
        let o = strs.len() as u64;
        strs.extend_from_slice(s);
        strs.push(0);
        o
    };
    let argo: Vec<u64> = argv.iter().map(|s| put(s.as_bytes())).collect();
    let envo: Vec<u64> = envp.iter().map(|s| put(s.as_bytes())).collect();
    let execfn = put(path.as_bytes());
    let plat = put(b"x86_64");
    let rnd = strs.len() as u64;
    let r = crate::syscall::random_u64().to_le_bytes();
    let r2 = crate::syscall::random_u64().to_le_bytes();
    strs.extend_from_slice(&r);
    strs.extend_from_slice(&r2);
    let sbase = (STACK_TOP - 16 - strs.len() as u64) & !15;
    let mut words: Vec<u64> = vec![argv.len() as u64];
    words.extend(argo.iter().map(|o| sbase + o));
    words.push(0);
    words.extend(envo.iter().map(|o| sbase + o));
    words.push(0);
    for (k, v) in [
        (3u64, phdr),
        (4, phent as u64),
        (5, phnum as u64),
        (6, 4096),
        (7, interp_base),
        (8, 0),
        (9, prog_entry),
        (11, 0),
        (12, 0),
        (13, 0),
        (14, 0),
        (16, 0x178bfbff),
        (17, 100),
        (23, 0),
        (25, sbase + rnd),
        (31, sbase + execfn),
        (15, sbase + plat),
        (51, 4096),
        (0, 0),
    ] {
        words.push(k);
        words.push(v);
        auxv.extend_from_slice(&k.to_le_bytes());
        auxv.extend_from_slice(&v.to_le_bytes());
    }
    let start = (sbase - words.len() as u64 * 8) & !15;
    let mut wb = Vec::with_capacity(words.len() * 8);
    for w in &words {
        wb.extend_from_slice(&w.to_le_bytes());
    }
    if STACK_TOP - start > 200 * 1024 {
        return Err("Argumente zu groß");
    }
    copy_to(&mut sp, start, &wb, true, false)?;
    copy_to(&mut sp, sbase, &strs, true, false)?;
    Ok(Image { auxv, space: sp, entry, sp: start, brk, brk_limit })
}

pub fn fresh_frame(entry: u64, sp: u64) -> TrapFrame {
    let mut f = TrapFrame::zeroed();
    f.fx[0] = 0x7f;
    f.fx[1] = 0x03;
    f.fx[24..28].copy_from_slice(&0x1f80u32.to_le_bytes());
    f.rip = entry;
    f.rsp = sp;
    f.cs = crate::arch::UCODE;
    f.ss = crate::arch::UDATA;
    f.rflags = 0x202;
    f
}

fn console_fds() -> Vec<Option<Fd>> {
    (0..3).map(|_| Some(Fd { kind: FdKind::Console, cloexec: false, flags: 2 })).collect()
}

/// Programm aus dem Dateibaum starten (Vordergrund der Kern-Konsole)
pub fn spawn(path: &str, argv: Vec<String>, envp: Vec<String>) -> Result<u64, String> {
    spawn_as(path, argv, envp, None, crate::groups::SESSION)
}

/// Dienst im Hintergrund starten (z. B. Entwickler-Zugang ultar-sshd): läuft neben Konsole und Oberfläche weiter;
/// `run_all` wartet nicht auf ihn und seine Nachkommen.
pub fn spawn_hintergrund(path: &str, argv: Vec<String>, envp: Vec<String>) -> Result<u64, String> {
    let pid = spawn_as(path, argv, envp, Some(new_pid()), crate::groups::SYSTEM)?;
    HINTERGRUND.lock().push(pid);
    Ok(pid)
}

/// Gruppen-Kennungen der Hintergrund-Dienste (Wurzeln; Nachkommen zählen mit)
static HINTERGRUND: Mutex<Vec<u64>> = Mutex::new(Vec::new());

fn ist_hintergrund(ps: &[Box<Proc>], p: &Proc, bg: &[u64]) -> bool {
    if bg.is_empty() {
        return false;
    }
    let (mut t, mut pp) = (p.tgid, p.ppid);
    for _ in 0..64 {
        if bg.contains(&t) {
            return true;
        }
        if pp == 0 {
            return false;
        }
        match ps.iter().find(|q| q.pid == pp) {
            Some(q) => {
                t = q.tgid;
                pp = q.ppid;
            }
            None => return false,
        }
    }
    false
}

/// läuft noch etwas im Vordergrund (nicht Hintergrund-Dienst, nicht beendet)?
fn vordergrund_aktiv() -> bool {
    let ps = PROCS.lock();
    let bg = HINTERGRUND.lock();
    ps.iter().any(|p| !matches!(p.state, State::Dead | State::Zombie(_)) && !ist_hintergrund(&ps, p, &bg))
}

fn hat_hintergrund() -> bool {
    !HINTERGRUND.lock().is_empty()
}

/// Hintergrund-Dienste ein Stück weiterlaufen lassen (Leerlauf der Kern-Konsole, Startseite)
pub fn hintergrund_schritt() {
    if !hat_hintergrund() || PROCS.lock().is_empty() {
        return;
    }
    reap_orphans();
    for _ in 0..8 {
        if run_one() != Some(true) {
            break;
        }
    }
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4()) };
}

/// nur den Vordergrund beenden (Strg+C doppelt): Hintergrund-Dienste laufen weiter
fn kill_vordergrund() {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4()) };
    let mut ps = PROCS.lock();
    let bg = HINTERGRUND.lock().clone();
    let groups: Vec<u64> = ps.iter().filter(|p| p.pid == p.tgid && !ist_hintergrund(&ps, p, &bg)).map(|p| p.tgid).collect();
    for g in groups {
        end_group(&mut ps, g, 9);
    }
}

/// Programm als PID 1 starten (ultar-init), wenn sonst nichts läuft (Hintergrund-Dienste ausgenommen)
pub fn spawn_init(path: &str, argv: Vec<String>, envp: Vec<String>) -> Result<u64, String> {
    if vordergrund_aktiv() || PROCS.lock().iter().any(|p| p.pid == 1) {
        return Err(String::from("es laufen noch Prozesse"));
    }
    spawn_as(path, argv, envp, Some(1), crate::groups::SYSTEM)
}

/// Start a program from the kernel itself in resource group `res`: programs of the kernel console
/// and the self test belong to `session`, `ultar-init` and background services to `system`.
/// Everything the new process needs is charged to that group.
fn spawn_as(path: &str, argv: Vec<String>, envp: Vec<String>, want: Option<u64>, res: u16) -> Result<u64, String> {
    let made = crate::heap::scope(res, || spawn_in(path, argv, envp, want, res));
    let p = made?;
    let pid = p.pid;
    PROCS.lock().push(p);
    Ok(pid)
}

fn spawn_in(path: &str, argv: Vec<String>, envp: Vec<String>, want: Option<u64>, res: u16) -> Result<Box<Proc>, String> {
    let data = fs::read_all(path).ok_or_else(|| alloc::format!("{path}: nicht gefunden"))?;
    let img = load_elf(&data, &argv, &envp, path, &|p| fs::read_all(p)).map_err(|e| alloc::format!("{path}: {e}"))?;
    let kstack = crate::mem::try_vec(KSTACK).ok_or_else(|| alloc::format!("{path}: {NO_MEMORY}"))?;
    let pids = crate::groups::PidCharge::new(res).ok_or_else(|| alloc::format!("{path}: zu viele Prozesse"))?;
    let pid = want.unwrap_or_else(|| NEXT_PID.fetch_add(1, Ordering::Relaxed));
    let g = Group {
        space: Some(img.space),
        fds: console_fds(),
        cwd: {
            let c = SHELL_CWD.lock().clone();
            if c.is_empty() { String::from("/") } else { c }
        },
        brk_start: img.brk,
        brk: img.brk,
        brk_limit: img.brk_limit,
        mmap_next: MMAP_BASE,
        umask: 0o22,
        exe: String::from(path),
        cmdline: argv.clone(),
        sigact: [SigAct::default(); 65],
        auxv: img.auxv.clone(),
        iso: crate::isolation::Isolation::host(),
        exe_host: String::from(path),
        res,
    };
    let p = Proc {
        pid,
        tgid: pid,
        ppid: 0,
        pgid: pid,
        name: argv.first().cloned().unwrap_or_default(),
        state: State::Ready,
        frame: Box::new(fresh_frame(img.entry, img.sp)),
        kstack,
        fs_base: 0,
        clear_tid: 0,
        sigmask: 0,
        pending: 0,
        sus_mask: None,
        killed: None,
        on_cpu: false,
        poll_dl: None,
        g: Shared::new(g),
        link: crate::isolation::pid::PidLink::new(crate::isolation::pid::PidNs::initial(), pid),
        pids,
        io_done: 0,
    };
    Ok(Box::new(p))
}

pub fn new_pid() -> u64 {
    loop {
        let p = NEXT_PID.fetch_add(1, Ordering::Relaxed);
        if p > 1 {
            return p;
        }
    }
}

// ------------------------------------------------------------------ Planer

const R_SWITCH: u64 = 1;

fn kstack_top(p: &Proc) -> u64 {
    (p.kstack.as_ptr() as u64 + p.kstack.len() as u64) & !15
}

static LAST: [AtomicU64; crate::arch::MAXCPU] = [const { AtomicU64::new(0) }; crate::arch::MAXCPU];

/// Einen bereiten Faden auf diesem Kern laufen lassen. None: es gibt keine Prozesse; Some(false): nichts bereit.
/// Fäden einer Gruppe laufen immer nur auf einem Kern zugleich (gemeinsamer Adressraum ohne TLB-Abgleich).
fn run_one() -> Option<bool> {
    crate::oom::watch();
    let cpu = crate::percpu::id();
    let fp = {
        let mut ps = PROCS.lock();
        if ps.is_empty() {
            return None;
        }
        wake(&mut ps);
        let busy: Vec<u64> = ps.iter().filter_map(|p| match p.state {
            State::Running(c) if c != cpu => Some(p.tgid),
            _ => None,
        }).collect();
        let n = ps.len();
        let last = LAST[cpu].load(Ordering::Relaxed) as usize;
        let mut found = None;
        for k in 1..=n {
            let i = (last + k) % n;
            if ps[i].state == State::Ready && !ps[i].on_cpu && !busy.contains(&ps[i].tgid) {
                found = Some(i);
                break;
            }
        }
        let Some(i) = found else { return Some(false) };
        LAST[cpu].store(i as u64, Ordering::Relaxed);
        if let Some(st) = ps[i].killed {
            let t = ps[i].tgid;
            end_group(&mut ps, t, st);
            return Some(true);
        }
        if ps[i].g.space.is_none() {
            // Gruppe bereits beendet (Adressraum freigegeben) – Faden nicht mehr fortsetzen
            ps[i].state = State::Dead;
            return Some(true);
        }
        // offene Signale vor dem Fortsetzen zustellen
        let p = &mut ps[i];
        let mut fr = (*p.frame).clone();
        match crate::signal::deliver(p, &mut fr) {
            Ok(_) => *p.frame = fr,
            Err(sig) => {
                let t = p.tgid;
                end_group(&mut ps, t, sig);
                return Some(true);
            }
        }
        let p = &mut ps[i];
        let top = kstack_top(p);
        let fp = (top - core::mem::size_of::<TrapFrame>() as u64) & !15;
        p.state = State::Running(cpu);
        p.on_cpu = true;
        unsafe {
            core::ptr::write(fp as *mut TrapFrame, (*p.frame).clone());
            core::arch::asm!("cli");
            crate::percpu::me().kstack_top = top;
            crate::arch::set_kernel_stack(top);
            crate::paging::wrmsr(0xC000_0100, p.fs_base);
        }
        p.space().activate();
        crate::percpu::me().current = p.pid;
        crate::heap::set_context(p.g.res);
        fp
    };
    let _ = unsafe { ultar_resume(fp as *mut TrapFrame, sched_ctx()) };
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4());
    }
    let me = core::mem::replace(&mut crate::percpu::me().current, 0);
    crate::heap::set_context(crate::groups::ROOT);
    crate::heap::clear_refused();
    {
        let mut ps = PROCS.lock();
        if let Some(p) = ps.iter_mut().find(|p| p.pid == me) {
            p.on_cpu = false;
            if p.state == State::Running(cpu) {
                p.state = State::Ready;
            }
        }
    }
    unsafe { core::arch::asm!("sti") };
    Some(true)
}

/// Alle Prozesse laufen lassen, bis keiner mehr lebt (Vordergrund der Kern-Konsole). Strg+C: SIGINT, zweimal: beenden.
pub fn run_all() {
    INTERRUPT.store(false, Ordering::Relaxed);
    loop {
        reap_orphans();
        let ran = match run_one() {
            None => break,
            Some(r) => r,
        };
        // nur noch Hintergrund-Dienste übrig: zurück zur Konsole/Startseite (sie laufen dort weiter)
        if hat_hintergrund() && !vordergrund_aktiv() {
            break;
        }
        crate::syscall::poll_input();
        crate::socket::poll();
        if INTERRUPT.swap(false, Ordering::Relaxed) {
            crate::kprintln!("^C");
            let now = crate::arch::uptime_ms();
            if now.saturating_sub(LAST_INT.swap(now, Ordering::Relaxed)) < 1000 {
                if hat_hintergrund() { kill_vordergrund() } else { kill_all() }
            } else {
                crate::signal::interrupt_all();
            }
        }
        if !ran {
            unsafe { core::arch::asm!("sti; hlt") };
        }
    }
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4()) };
}

/// Planer der weiteren Kerne
pub fn ap_loop() -> ! {
    loop {
        match run_one() {
            Some(true) => {}
            _ => unsafe { core::arch::asm!("sti; hlt") },
        }
    }
}

fn wake(ps: &mut [Box<Proc>]) {
    let now = crate::arch::uptime_ms();
    let input = crate::syscall::input_ready();
    let all: Vec<(u64, u64, bool)> = ps.iter().filter(|p| p.pid == p.tgid).map(|p| (p.pid, p.ppid, matches!(p.state, State::Zombie(_)))).collect();
    let alive: Vec<u64> = all.iter().filter(|x| !x.2).map(|x| x.0).collect();
    for p in ps.iter_mut() {
        if let State::Blocked(w) = p.state {
            let go = match w {
                Wait::Input => input,
                Wait::Sleep(t) => now >= t,
                Wait::Pipe => true,
                Wait::Futex(_, t) => {
                    if t != 0 && now >= t {
                        p.frame.rax = (-110i64) as u64;
                        true
                    } else {
                        false
                    }
                }
                Wait::Signal => {
                    if p.pending & !p.sigmask != 0 {
                        p.frame.rax = (-4i64) as u64;
                        true
                    } else {
                        false
                    }
                }
                Wait::Net(g) => crate::socket::gen() != g,
                Wait::Poll(g, t) => crate::socket::gen() != g || input || (t != 0 && now >= t) || p.pending & !p.sigmask != 0 || p.pending & p.sigmask != 0 && has_sigfd(p),
                Wait::Child(pid) => {
                    let mine = |x: &&(u64, u64, bool)| x.1 == p.tgid && (pid <= 0 || x.0 == pid as u64);
                    all.iter().filter(mine).any(|x| x.2) || !all.iter().any(|x| mine(&x))
                }
                Wait::Vfork(c) => !alive.contains(&c),
            };
            if go {
                p.state = State::Ready;
            }
        }
    }
}

fn has_sigfd(p: &Proc) -> bool {
    p.g.fds.iter().flatten().any(|f| matches!(f.kind, FdKind::SigFd(_)))
}

fn reap_orphans() {
    let mut ps = PROCS.lock();
    let pids: Vec<u64> = ps.iter().map(|p| p.pid).collect();
    ps.retain(|p| p.on_cpu || !(p.state == State::Dead || matches!(p.state, State::Zombie(_)) && (p.ppid == 0 || !pids.contains(&p.ppid))));
}

pub fn kill_all() {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4()) };
    let mut ps = PROCS.lock();
    let groups: Vec<u64> = ps.iter().filter(|p| p.pid == p.tgid).map(|p| p.tgid).collect();
    for g in groups {
        end_group(&mut ps, g, 9);
    }
    ps.retain(|p| p.on_cpu || matches!(p.state, State::Running(_)) || p.killed.is_some() && !matches!(p.state, State::Zombie(_) | State::Dead));
}

/// Prozess anhalten und zurück in den Planer (Rahmen wird gesichert). Keine Sperre darf gehalten werden.
pub fn switch_out(f: &TrapFrame, state: Option<State>) -> ! {
    {
        let mut ps = PROCS.lock();
        let me = current();
        if let Some(p) = ps.iter_mut().find(|p| p.pid == me) {
            *p.frame = f.clone();
            p.fs_base = unsafe { crate::paging::rdmsr(0xC000_0100) };
            p.state = state.unwrap_or(State::Ready);
        }
    }
    unsafe { ultar_back(sched_ctx(), R_SWITCH) }
}

/// Fadengruppe beenden: alle Fäden, Speicher und Dateien freigeben, Gruppenführer wird Zombie
fn end_group(ps: &mut Vec<Box<Proc>>, tgid: u64, status: i32) {
    let cpu = crate::percpu::id();
    if ps.iter().any(|t| t.tgid == tgid && matches!(t.state, State::Running(c) if c != cpu)) {
        // läuft auf einem anderen Kern: dort beim nächsten Eintritt beenden
        for t in ps.iter_mut().filter(|t| t.tgid == tgid) {
            t.killed = Some(status);
        }
        return;
    }
    let mut g = None;
    for t in ps.iter_mut().filter(|t| t.tgid == tgid) {
        t.state = if t.pid == tgid { State::Zombie(status) } else { State::Dead };
        g = Some(t.g.clone());
    }
    if let Some(mut g) = g {
        let fds = core::mem::take(&mut g.fds);
        let space = g.space.take();
        drop(fds);
        drop(space);
    }
    for c in ps.iter_mut().filter(|c| c.ppid == tgid) {
        c.ppid = 0;
    }
    end_pid_namespace(ps, tgid);
    // Elternprozess benachrichtigen (SIGCHLD); vfork-Eltern und wait4-Wartende weckt der Planer
    if let Some(pp) = ps.iter().find(|t| t.pid == tgid).map(|t| t.ppid) {
        if pp != 0 {
            crate::signal::send_locked(ps, pp, 0, crate::signal::SIGCHLD);
        }
    }
}

/// If the group that just ended was pid 1 of a pid namespace: close the namespace and kill every
/// other process in it (and in namespaces nested below).
fn end_pid_namespace(ps: &mut Vec<Box<Proc>>, tgid: u64) {
    let Some(ns) = ps.iter().find(|t| t.pid == tgid).map(|t| t.link.ns().clone()) else { return };
    if ns.is_initial() || ns.init_global() != tgid || ns.is_dead() {
        return;
    }
    ns.mark_dead();
    let victims: Vec<u64> = ps
        .iter()
        .filter(|p| p.pid == p.tgid && p.tgid != tgid && ns.contains(p.link.ns()) && !matches!(p.state, State::Zombie(_) | State::Dead))
        .map(|p| p.tgid)
        .collect();
    for v in victims {
        kill_group(ps, v, 9);
    }
}

/// Faden nach Linux-Regeln beenden; `group`: exit_group (alle Fäden)
pub fn exit_thread(status: i32, group: bool) -> ! {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4()) };
    {
        let mut ps = PROCS.lock();
        let me = current();
        if let Some(i) = ps.iter().position(|p| p.pid == me) {
            let tgid = ps[i].tgid;
            // CLONE_CHILD_CLEARTID: 0 schreiben und einen Wartenden wecken (pthread_join)
            let ct = ps[i].clear_tid;
            if ct != 0 {
                if let Some(sp) = ps[i].g.space.as_ref() {
                    write_user(sp, ct, &0u32.to_le_bytes());
                    if let Some((pa, _)) = sp.translate(ct) {
                        futex_wake_locked(&mut ps, pa, 1);
                    }
                }
            }
            let others = ps.iter().filter(|t| t.tgid == tgid && t.pid != me && !matches!(t.state, State::Dead | State::Exiting | State::Zombie(_))).count();
            if group || others == 0 {
                end_group(&mut ps, tgid, status);
            } else {
                ps[i].state = if me == tgid { State::Exiting } else { State::Dead };
            }
        }
    }
    unsafe { ultar_back(sched_ctx(), R_SWITCH) }
}

/// Prozess beenden (Status wie Linux: Code << 8 oder Signal)
pub fn exit_current(status: i32) -> ! {
    exit_thread(status, true)
}

/// Gruppe von außen beenden (Signal)
pub fn kill_group(ps: &mut Vec<Box<Proc>>, tgid: u64, status: i32) {
    // trifft es die eigene Gruppe mitten in einem Systemaufruf (z. B. abort() → kill(getpid(), SIGABRT)),
    // nicht sofort freigeben: der laufende Aufruf braucht den Adressraum noch – beendet wird beim Rücksprung
    let me = current();
    if ps.iter().any(|t| t.tgid == tgid && t.pid == me) {
        for t in ps.iter_mut().filter(|t| t.tgid == tgid) {
            t.killed = Some(status);
        }
        return;
    }
    end_group(ps, tgid, status);
}

/// bis zu `n` Fäden wecken, die auf die Futex an `pa` warten
pub fn futex_wake_locked(ps: &mut [Box<Proc>], pa: u64, n: u64) -> u64 {
    let mut k = 0;
    for p in ps.iter_mut() {
        if k >= n {
            break;
        }
        if let State::Blocked(Wait::Futex(a, _)) = p.state {
            if a == pa {
                p.state = State::Ready;
                p.frame.rax = 0;
                k += 1;
            }
        }
    }
    k
}

/// Zeitgeber aus einem Benutzerprogramm: anderen Prozessen Rechenzeit geben
pub fn on_timer(f: &mut TrapFrame) {
    let me = current();
    let killed = PROCS.lock().iter().find(|p| p.pid == me).and_then(|p| p.killed);
    if let Some(st) = killed {
        exit_thread(st, true);
    }
    // Zeitscheibe 20 ms (jeder zweite Takt dieses Kerns)
    if crate::arch::uptime_ms() / 10 % 2 != 0 {
        return;
    }
    let (others, sig) = {
        let ps = PROCS.lock();
        let o = ps.iter().filter(|p| p.state == State::Ready && !p.on_cpu).count();
        let s = ps.iter().find(|p| p.pid == me).map(|p| p.pending & !p.sigmask != 0).unwrap_or(false);
        (o, s)
    };
    if sig {
        switch_out(f, None);
    }
    if crate::percpu::id() == 0 {
        crate::socket::poll();
    }
    crate::syscall::poll_input();
    if INTERRUPT.load(Ordering::Relaxed) {
        switch_out(f, None);
    }
    if others > 0 {
        switch_out(f, None);
    }
}

/// Ausnahme in einem Benutzerprogramm: Stapel wachsen lassen oder Prozess beenden
pub fn on_fault(f: &mut TrapFrame, v: u64) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2) };
    crate::heap::clear_refused();
    if v == 14 && f.err & 1 == 0 {
        // reservierter mmap-Bereich oder Stapel: Seite jetzt belegen
        use crate::paging::Fault;
        let got = {
            let mut ps = PROCS.lock();
            let me = current();
            match ps.iter_mut().find(|p| p.pid == me) {
                Some(p) => {
                    crate::heap::set_context(p.g.res);
                    match p.g.space.as_ref().map(|s| s.fault_in(cr2)) {
                        Some(Fault::NotReserved) if cr2 < STACK_TOP && cr2 >= STACK_TOP - STACK_MAX => match p.space_mut().map(cr2 & !0xfff, true, false) {
                            Some(_) => Fault::Mapped,
                            None => Fault::NoMemory,
                        },
                        Some(r) => r,
                        None => Fault::NotReserved,
                    }
                }
                None => Fault::NotReserved,
            }
        };
        match got {
            Fault::Mapped => return,
            // no page to be had: the process ends or tries again later – the machine keeps running
            Fault::NoMemory => crate::oom::fault(f),
            Fault::NotReserved => {}
        }
    }
    let sig = match v {
        0 | 16 | 19 => 8,
        6 => 4,
        _ => 11,
    };
    {
        let mut ps = PROCS.lock();
        let me = current();
        if let Some(p) = ps.iter_mut().find(|p| p.pid == me) {
            if p.g.sigact[sig as usize].handler > 1 && p.sigmask & (1 << (sig - 1)) == 0 {
                p.pending |= 1 << (sig - 1);
                if let Ok(true) = crate::signal::deliver(p, f) {
                    return;
                }
            }
        }
    }
    // built and freed in an inner scope: nothing on this stack is dropped once the process is gone
    crate::heap::kernel(|| {
        let name = PROCS.lock().iter().find(|p| p.pid == current()).map(|p| p.name.clone()).unwrap_or_default();
        crate::kprintln!("[proc] {} (pid {}): {} bei {:#x}, Adresse {:#x} – beendet", name, current(), crate::trap::name(v), f.rip, cr2);
    });
    exit_current(sig as i32);
}

pub fn on_syscall(f: &mut TrapFrame) {
    let me = current();
    let (killed, res) = PROCS.lock().iter().find(|p| p.pid == me).map(|p| (p.killed, p.g.res)).unwrap_or((None, crate::groups::ROOT));
    if let Some(st) = killed {
        exit_thread(st, true);
    }
    // everything the call allocates is charged to the caller's group
    crate::heap::set_context(res);
    crate::heap::clear_refused();
    crate::percpu::me().errno = 0;
    // only where a `memory.max` is set somewhere: remember the charge to see what this call added
    let before = if crate::groups::limits_set() { crate::groups::memory(res) } else { u64::MAX };
    crate::syscall::dispatch(f);
    crate::oom::after_syscall(res, before);
    let killed = PROCS.lock().iter().find(|p| p.pid == me).and_then(|p| p.killed);
    if let Some(st) = killed {
        exit_thread(st, true);
    }
}

pub struct Info {
    /// Kernel-wide number of the group (`pid`, `ppid`, `pgid` are the numbers the reader sees)
    pub global: u64,
    pub pid: u64,
    pub ppid: u64,
    pub pgid: u64,
    pub name: String,
    pub cmdline: Vec<String>,
    pub exe: String,
    pub state: State,
    pub pages: u64,
    pub auxv: Vec<u8>,
    /// Resource group of the process
    pub res: u16,
}

/// Prozesse, wie der lesende Prozess sie sieht: only those visible in the reader's pid namespace, with the
/// reader's numbers. Called from the kernel console (no current process) it lists everything.
pub fn infos() -> Vec<Info> {
    let ps = PROCS.lock();
    let me = current();
    let ns = ps.iter().find(|p| p.pid == me).map(|p| p.link.ns().clone()).unwrap_or_else(crate::isolation::pid::PidNs::initial);
    ps.iter()
        .filter(|p| p.pid == p.tgid)
        .filter_map(|p| {
            let pid = ns.local(p.pid)?;
            let pages = p.g.space.as_ref().map(|s| s.resident.load(Ordering::Relaxed)).unwrap_or(0);
            let (ppid, pgid) = (ns.local(p.ppid).unwrap_or(0), ns.local(p.pgid).unwrap_or(0));
            Some(Info { global: p.pid, pid, ppid, pgid, name: p.name.clone(), cmdline: p.g.cmdline.clone(), exe: p.g.exe_host.clone(), state: p.state, pages, auxv: p.g.auxv.clone(), res: p.g.res })
        })
        .collect()
}

/// Group (kernel-wide number) of the calling thread; 0 outside a process.
pub fn current_group() -> u64 {
    let me = current();
    PROCS.lock().iter().find(|p| p.pid == me).map(|p| p.tgid).unwrap_or(0)
}

/// Whether the reader lives in the initial pid namespace (the kernel console counts as that).
pub fn reader_in_initial_pid_ns() -> bool {
    let me = current();
    PROCS.lock().iter().find(|p| p.pid == me).map(|p| p.link.ns().is_initial()).unwrap_or(true)
}

/// Mount namespace of the calling process if it is not the initial one.
pub fn reader_mount_ns() -> Option<Arc<crate::isolation::mount::MountNs>> {
    let me = current();
    PROCS.lock().iter().find(|p| p.pid == me).map(|p| p.g.iso.mnt.clone()).filter(|m| !m.is_initial())
}

/// Namespace of kind `kind` ("mnt" or "pid") of the group with the kernel-wide number `tgid`.
pub fn ns_ref(tgid: u64, kind: &str) -> Option<crate::isolation::NsRef> {
    let ps = PROCS.lock();
    let p = ps.iter().find(|p| p.pid == tgid)?;
    match kind {
        "mnt" => Some(crate::isolation::NsRef::Mnt(p.g.iso.mnt.clone())),
        "pid" => Some(crate::isolation::NsRef::Pid(p.link.ns().clone())),
        _ => None,
    }
}

pub fn list() -> Vec<(u64, u64, String, State)> {
    PROCS.lock().iter().map(|p| (p.pid, p.ppid, p.name.clone(), p.state)).collect()
}

/// Process ids of the thread groups that belong to resource group `g` (not its descendants), as the
/// reader's pid namespace numbers them.
pub fn group_members(g: u16) -> Vec<u64> {
    let ps = PROCS.lock();
    let me = current();
    let ns = ps.iter().find(|p| p.pid == me).map(|p| p.link.ns().clone()).unwrap_or_else(crate::isolation::pid::PidNs::initial);
    ps.iter().filter(|p| p.pid == p.tgid && p.g.res == g && !matches!(p.state, State::Dead)).filter_map(|p| ns.local(p.pid)).collect()
}

/// Move the thread group `tgid` (kernel-wide number) into resource group `to`: its threads are
/// counted there from now on and new memory is charged there. Memory charged so far stays with the
/// old group until it is freed. `Err`: negative error number (`ESRCH`, `EAGAIN` if `to` or an
/// ancestor has no room for the threads, `ENOENT` if the group is gone).
pub fn move_group(tgid: u64, to: u16) -> Result<(), i64> {
    let mut ps = PROCS.lock();
    if !crate::groups::is_live(to) {
        return Err(-2);
    }
    let from = match ps.iter().find(|p| p.tgid == tgid && !matches!(p.state, State::Dead)) {
        Some(p) => p.g.res,
        None => return Err(-3),
    };
    let mut moved = 0;
    let mut failed = false;
    for p in ps.iter_mut().filter(|p| p.tgid == tgid) {
        if p.pids.move_to(to).is_err() {
            failed = true;
            break;
        }
        moved += 1;
    }
    if failed {
        // all or nothing: put the threads moved so far back (their old places are still free)
        for p in ps.iter_mut().filter(|p| p.tgid == tgid).take(moved) {
            let _ = p.pids.move_to(from);
        }
        return Err(-11);
    }
    if let Some(p) = ps.iter_mut().find(|p| p.tgid == tgid) {
        p.g.res = to;
    }
    if tgid == ps.iter().find(|p| p.pid == current()).map(|p| p.tgid).unwrap_or(0) {
        crate::heap::set_context(to);
    }
    Ok(())
}
