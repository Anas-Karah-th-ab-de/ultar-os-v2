//! The engine in another process: the WPE WebKit host (`external/wpe-webkit/host`) started
//! in its own cell. One Unix stream socket (the host's standard input) carries the protocol
//! of [`crate::ipc`]; frames are shared memory the host passes with `SCM_RIGHTS`. The browser
//! reads only the damaged rows of a frame (`pread`, no mapping) into its own copy and then
//! tells the host it may render again.

use super::{
    DownloadId, EngineError, EngineEvent, FrameView, KeyInput, PointerInput, RequestId, TabId,
    WebEngine, MAX_TABS,
};
use crate::cell::CellSpec;
use crate::ipc::{Command, Decoder, HostEvent, MAX_MESSAGE, PROTOCOL_VERSION};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use rustix::net::{
    recvmsg, send, socketpair, AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags,
    SendFlags, SocketFlags, SocketType,
};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::fs::File;
use std::io::IoSliceMut;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::process::{Child, Command as Process, Stdio};
use std::time::{Duration, Instant};

/// Largest frame buffer accepted from the host (bytes).
pub const MAX_BUFFER: u64 = 256 * 1024 * 1024;
/// Most descriptors waiting for their buffer message.
const MAX_PENDING_FDS: usize = 8;
/// How long the host may take to say it is ready.
pub const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// How to start the host.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// The host program (`<bundle>/libexec/ultar-web-engine`).
    pub program: PathBuf,
    /// The cell; `None` only for development (`--no-cell`), never in a release.
    pub cell: Option<CellSpec>,
    /// Profile folder (cookies, cache, local storage).
    pub profile: PathBuf,
    /// Downloads folder.
    pub downloads: PathBuf,
    /// User agent product token (from `ultar-brand`).
    pub user_agent: String,
    /// Compiled content-blocker list (`ultar-web-filter` output), if any.
    pub filter: Option<PathBuf>,
    /// The only way out to the internet for the engine: the browser's Tor port for HTTP
    /// `CONNECT` with its secret (`http://user:password@127.0.0.1:port`). An engine host given
    /// a proxy sends nothing past it.
    pub proxy: Option<String>,
    /// The way out of an engine in a cell **without network**: the Unix socket of the browser's
    /// Tor client (`ultar_tor::Tor::unix_proxy`, in the engine's folder, see
    /// `cell::proxy_socket`). Used with a cell instead of `proxy`; the host's relay inside the
    /// cell leads Servo to it.
    pub proxy_socket: Option<PathBuf>,
}

/// The environment of the host (it starts with an empty one): the time zone every page sees
/// (`privacy::persona`), never the device's.
pub const HOST_ENV: [(&str, &str); 2] = [("TZ", "UTC"), ("LANG", "en_US.UTF-8")];

impl HostConfig {
    /// The host's arguments.
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "--profile".into(),
            self.profile.clone().into_os_string(),
            "--downloads".into(),
            self.downloads.clone().into_os_string(),
            "--user-agent".into(),
            self.user_agent.clone().into(),
        ];
        if let Some(filter) = &self.filter {
            args.push("--filter".into());
            args.push(filter.clone().into_os_string());
        }
        if let Some(proxy) = &self.proxy {
            args.push("--proxy".into());
            args.push(proxy.into());
        }
        if let Some(socket) = &self.proxy_socket {
            args.push("--proxy-socket".into());
            args.push(socket.clone().into_os_string());
        }
        match &self.cell {
            Some(cell) => args.extend(cell.args()),
            None => args.push("--no-cell".into()),
        }
        args
    }
}

#[derive(Debug)]
struct Buffer {
    file: File,
    width: u32,
    height: u32,
    stride: u32,
    pixels: Vec<u8>,
}

/// The engine host as a [`WebEngine`].
#[derive(Debug)]
pub struct RemoteEngine {
    child: Option<Child>,
    socket: OwnedFd,
    decoder: Decoder,
    fds: VecDeque<OwnedFd>,
    buffers: BTreeMap<TabId, Buffer>,
    tabs: BTreeMap<TabId, bool>,
    next_tab: u32,
}

impl RemoteEngine {
    /// Starts the host and waits until it is ready.
    pub fn start(config: &HostConfig) -> Result<RemoteEngine, EngineError> {
        let (ours, theirs) = socketpair(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::CLOEXEC,
            None,
        )
        .map_err(|e| EngineError::Start(e.into()))?;
        let child = Process::new(&config.program)
            .args(config.args())
            .env_clear()
            .envs(HOST_ENV)
            .stdin(Stdio::from(theirs))
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(EngineError::Start)?;
        let mut engine = RemoteEngine::with_socket(ours);
        engine.child = Some(child);
        engine.wait_ready(READY_TIMEOUT)?;
        Ok(engine)
    }

    /// An engine over an already connected socket (tests, a host started elsewhere).
    pub fn with_socket(socket: OwnedFd) -> RemoteEngine {
        RemoteEngine {
            child: None,
            socket,
            decoder: Decoder::default(),
            fds: VecDeque::new(),
            buffers: BTreeMap::new(),
            tabs: BTreeMap::new(),
            next_tab: 0,
        }
    }

    /// Waits for the host's `Ready` with the right version.
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), EngineError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(EngineError::Disconnected);
            }
            if !self.receive(Some(remaining))? {
                continue;
            }
            if let Some((kind, payload)) =
                self.decoder.next_message().map_err(EngineError::Protocol)?
            {
                return match HostEvent::decode(kind, &payload).map_err(EngineError::Protocol)? {
                    HostEvent::Ready(PROTOCOL_VERSION) => Ok(()),
                    _ => Err(EngineError::Disconnected),
                };
            }
        }
    }

    fn command(&mut self, command: &Command) -> Result<(), EngineError> {
        let bytes = command.encode();
        let mut sent = 0;
        while sent < bytes.len() {
            match send(&self.socket, &bytes[sent..], SendFlags::NOSIGNAL) {
                Ok(0) => return Err(EngineError::Disconnected),
                Ok(n) => sent += n,
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => {
                    let mut fds = [PollFd::new(&self.socket, PollFlags::OUT)];
                    poll(&mut fds, None).map_err(|e| EngineError::Io(e.into()))?;
                }
                Err(_) => return Err(EngineError::Disconnected),
            }
        }
        Ok(())
    }

    fn tab_command(&mut self, tab: TabId, command: Command) -> Result<(), EngineError> {
        if !self.tabs.contains_key(&tab) {
            return Err(EngineError::UnknownTab(tab));
        }
        self.command(&command)
    }

    /// Reads what the socket has (waiting up to `timeout` for the first bytes). Returns
    /// `false` on timeout.
    fn receive(&mut self, timeout: Option<Duration>) -> Result<bool, EngineError> {
        let spec = timeout.map(|t| Timespec {
            tv_sec: t.as_secs().min(i64::MAX as u64) as i64,
            tv_nsec: i64::from(t.subsec_nanos()),
        });
        let mut fds = [PollFd::new(&self.socket, PollFlags::IN)];
        match poll(&mut fds, spec.as_ref()) {
            Ok(0) => return Ok(false),
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => return Ok(false),
            Err(e) => return Err(EngineError::Io(e.into())),
        }
        let mut got = false;
        loop {
            let mut data = [0u8; 16 * 1024];
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let mut iov = [IoSliceMut::new(&mut data)];
            let flags = RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC;
            let n = match recvmsg(&self.socket, &mut iov, &mut control, flags) {
                Ok(msg) => msg.bytes,
                Err(rustix::io::Errno::AGAIN) => return Ok(got),
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return Err(EngineError::Io(e.into())),
            };
            for message in control.drain() {
                if let RecvAncillaryMessage::ScmRights(received) = message {
                    for fd in received {
                        if self.fds.len() >= MAX_PENDING_FDS {
                            return Err(EngineError::Limit);
                        }
                        self.fds.push_back(fd);
                    }
                }
            }
            if n == 0 {
                return Err(EngineError::Disconnected);
            }
            if self.decoder.pending() + n > 4 * MAX_MESSAGE {
                return Err(EngineError::Limit);
            }
            self.decoder.push(&data[..n]);
            got = true;
        }
    }

    fn take_buffer(
        &mut self,
        tab: TabId,
        width: u32,
        height: u32,
        stride: u32,
    ) -> Result<(), EngineError> {
        let fd = self.fds.pop_front().ok_or(EngineError::Disconnected)?;
        let file = File::from(fd);
        let size = u64::from(stride) * u64::from(height);
        let len = file.metadata().map_err(EngineError::Io)?.len();
        if size > MAX_BUFFER || len < size || !self.tabs.contains_key(&tab) {
            return Err(EngineError::Limit);
        }
        let pixels = vec![0u8; size as usize];
        self.buffers.insert(
            tab,
            Buffer {
                file,
                width,
                height,
                stride,
                pixels,
            },
        );
        Ok(())
    }

    fn read_frame(&mut self, tab: TabId, d: super::FrameDamage) -> Result<bool, EngineError> {
        let Some(b) = self.buffers.get_mut(&tab) else {
            return Ok(false);
        };
        let (x1, y1) = (d.x1.min(b.width), d.y1.min(b.height));
        if d.x0 >= x1 || d.y0 >= y1 {
            return Ok(false);
        }
        let stride = b.stride as usize;
        for y in d.y0..y1 {
            let start = y as usize * stride + d.x0 as usize * 4;
            let end = y as usize * stride + x1 as usize * 4;
            let row = b.pixels.get_mut(start..end).ok_or(EngineError::Limit)?;
            b.file
                .read_exact_at(row, start as u64)
                .map_err(EngineError::Io)?;
        }
        Ok(true)
    }

    fn handle(
        &mut self,
        kind: u8,
        payload: &[u8],
        out: &mut Vec<EngineEvent>,
    ) -> Result<(), EngineError> {
        match HostEvent::decode(kind, payload).map_err(EngineError::Protocol)? {
            HostEvent::Ready(_) => Err(EngineError::Disconnected),
            HostEvent::Buffer(tab, w, h, stride) => self.take_buffer(tab, w, h, stride),
            HostEvent::Engine(EngineEvent::Frame { tab, damage }) => {
                let read = self.read_frame(tab, damage)?;
                if self.tabs.contains_key(&tab) {
                    self.command(&Command::FrameDone(tab))?;
                }
                if read {
                    out.push(EngineEvent::Frame { tab, damage });
                }
                Ok(())
            }
            HostEvent::Engine(event) => {
                out.push(event);
                Ok(())
            }
        }
    }
}

impl WebEngine for RemoteEngine {
    fn create_tab(&mut self, private: bool) -> Result<TabId, EngineError> {
        if self.tabs.len() >= MAX_TABS {
            return Err(EngineError::Limit);
        }
        self.next_tab += 1;
        let tab = TabId(self.next_tab);
        self.command(&Command::CreateTab(tab, private))?;
        self.tabs.insert(tab, private);
        Ok(tab)
    }

    fn close_tab(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tab_command(tab, Command::CloseTab(tab))?;
        self.tabs.remove(&tab);
        self.buffers.remove(&tab);
        Ok(())
    }

    fn load(&mut self, tab: TabId, url: &str) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Load(tab, url.to_string()))
    }

    fn back(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Back(tab))
    }

    fn forward(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Forward(tab))
    }

    fn reload(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Reload(tab))
    }

    fn stop(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Stop(tab))
    }

    fn resize(
        &mut self,
        tab: TabId,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Resize(tab, width, height, scale))
    }

    fn set_visible(&mut self, tab: TabId, visible: bool) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Visible(tab, visible))
    }

    fn pointer(&mut self, tab: TabId, input: PointerInput) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Pointer(tab, input))
    }

    fn key(&mut self, tab: TabId, input: KeyInput) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Key(tab, input))
    }

    fn set_protection(&mut self, tab: TabId, on: bool) -> Result<(), EngineError> {
        self.tab_command(tab, Command::Protection(tab, on))
    }

    fn answer_permission(&mut self, request: RequestId, allow: bool) -> Result<(), EngineError> {
        self.command(&Command::Permission(request, allow))
    }

    fn cancel_download(&mut self, download: DownloadId) -> Result<(), EngineError> {
        self.command(&Command::CancelDownload(download))
    }

    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<EngineEvent>, EngineError> {
        let mut out = Vec::new();
        self.receive(timeout)?;
        while let Some((kind, payload)) =
            self.decoder.next_message().map_err(EngineError::Protocol)?
        {
            self.handle(kind, &payload, &mut out)?;
        }
        Ok(out)
    }

    fn frame(&self, tab: TabId) -> Option<FrameView<'_>> {
        let b = self.buffers.get(&tab)?;
        Some(FrameView {
            width: b.width,
            height: b.height,
            stride: b.stride as usize,
            pixels: &b.pixels,
        })
    }

    fn wake_fd(&self) -> Option<BorrowedFd<'_>> {
        Some(self.socket.as_fd())
    }
}

impl Drop for RemoteEngine {
    fn drop(&mut self) {
        let _ = self.command(&Command::Quit);
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
