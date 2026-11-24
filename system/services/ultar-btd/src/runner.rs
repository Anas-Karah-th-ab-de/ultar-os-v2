//! The stack thread: owns the transport and the [`Host`], feeds it packets, wishes and
//! the time, saves bonds, hands input to the sink and leaves a snapshot for the service.
//!
//! Transports: Linux – the HCI user channel of the first controller; Ultar – the raw
//! HCI node of the first USB controller (`dev/ultar/hciN`, kernel right `bluetooth`).
//! Both in `ultar_hal_linux::bluetooth`; with neither the service says "no device".
//! Input sink: virtual devices through `uinput` (Linux `dev/uinput`, Ultar
//! `dev/ultar/uinput`, right `input_inject`).

use crate::stack::bonds::{Bonds, Kind};
use crate::stack::hci::Addr;
use crate::stack::hid::{InputEvent, InputKind, InputSink};
use crate::stack::host::{Cmd, Config, Host, Refusal, Up};
use crate::stack::smp::Ask;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Questions waiting for the system dialog at most (more: refused).
pub const MAX_ASKS: usize = 4;

/// A way to the controller (H4 packets).
pub trait Transport: Send {
    /// One packet, waiting at most `timeout`; `None` if none came.
    fn read(&mut self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>>;
    /// One packet out.
    fn write(&mut self, packet: &[u8]) -> std::io::Result<()>;
}

/// The Linux HCI user channel.
pub struct UserChannel(ultar_hal_linux::bluetooth::UserChannel);

impl Transport for UserChannel {
    fn read(&mut self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
        self.0.read(timeout)
    }
    fn write(&mut self, packet: &[u8]) -> std::io::Result<()> {
        self.0.write(packet)
    }
}

/// The raw HCI node of the Ultar kernel.
pub struct RawNode(ultar_hal_linux::bluetooth::RawNode);

impl Transport for RawNode {
    fn read(&mut self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
        self.0.read(timeout)
    }
    fn write(&mut self, packet: &[u8]) -> std::io::Result<()> {
        self.0.write(packet)
    }
}

/// The first controller's transport, if the kernel offers one: the Linux user channel,
/// else the first Ultar node that opens.
pub fn open(root: &Path) -> Option<Box<dyn Transport>> {
    use ultar_hal_linux::bluetooth as bt;
    if let Some(&dev) = bt::controllers(root).first() {
        if let Ok(ch) = bt::UserChannel::open(dev) {
            return Some(Box::new(UserChannel(ch)));
        }
    }
    bt::ultar_nodes(root)
        .iter()
        .find_map(|p| bt::RawNode::open(p).ok())
        .map(|n| Box::new(RawNode(n)) as Box<dyn Transport>)
}

/// A question for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// Token for the answer.
    pub token: u32,
    /// Device.
    pub addr: Addr,
    /// Its name.
    pub name: String,
    /// Its kind.
    pub kind: Kind,
    /// What to ask.
    pub ask: Ask,
}

/// One device in the snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceView {
    /// Address.
    pub addr: Addr,
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: Kind,
    /// RSSI.
    pub rssi: Option<i8>,
    /// LE.
    pub le: bool,
    /// Bonded.
    pub paired: bool,
    /// Connected.
    pub connected: bool,
    /// Input devices registered from it.
    pub input: bool,
}

/// What the service reads.
#[derive(Clone, Debug, Default)]
pub struct Shared {
    /// The thread runs and the controller is set up.
    pub ready: bool,
    /// The thread ended (transport gone).
    pub ended: bool,
    /// Scanning.
    pub scanning: bool,
    /// Devices (bonded first).
    pub devices: Vec<DeviceView>,
    /// Questions not yet handed to the dialog.
    pub questions: VecDeque<Question>,
    /// Ended pairings (address, reason; 0 = paired).
    pub results: VecDeque<(Addr, u8)>,
}

type Request = (Cmd, Sender<Result<(), Refusal>>);

/// The handle of the stack thread.
pub struct Stack {
    tx: Sender<Request>,
    /// The snapshot.
    pub shared: Arc<Mutex<Shared>>,
}

impl Stack {
    /// Starts the thread on a transport; bonds are kept in `bonds_file`.
    pub fn spawn(
        t: Box<dyn Transport>,
        bonds_file: PathBuf,
        sink: Box<dyn InputSink>,
        cfg: Config,
    ) -> Option<Self> {
        let (tx, rx) = channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let s2 = shared.clone();
        std::thread::Builder::new()
            .name("bt-stack".into())
            .stack_size(256 * 1024)
            .spawn(move || run(t, bonds_file, sink, cfg, rx, s2))
            .ok()?;
        Some(Self { tx, shared })
    }

    /// A wish; the host's answer within 2 s (else `NotReady`).
    pub fn command(&self, c: Cmd) -> Result<(), Refusal> {
        let (rtx, rrx) = channel();
        self.tx.send((c, rtx)).map_err(|_| Refusal::NotReady)?;
        rrx.recv_timeout(Duration::from_secs(2))
            .unwrap_or(Err(Refusal::NotReady))
    }

    /// A copy of the snapshot.
    pub fn snapshot(&self) -> Shared {
        self.shared.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Takes the waiting questions and ended pairings.
    pub fn take_news(&self) -> (Vec<Question>, Vec<(Addr, u8)>) {
        match self.shared.lock() {
            Ok(mut s) => (
                s.questions.drain(..).collect(),
                s.results.drain(..).collect(),
            ),
            Err(_) => (vec![], vec![]),
        }
    }
}

fn run(
    mut t: Box<dyn Transport>,
    bonds_file: PathBuf,
    mut sink: Box<dyn InputSink>,
    cfg: Config,
    rx: Receiver<Request>,
    shared: Arc<Mutex<Shared>>,
) {
    let start = Instant::now();
    let mut host = Host::new(cfg, Bonds::load(&bonds_file));
    let mut inputs: Vec<u32> = Vec::new();
    loop {
        for p in host.take_out() {
            if t.write(&p).is_err() {
                return end(&shared, &mut sink, &inputs);
            }
        }
        match t.read(Duration::from_millis(50)) {
            Ok(Some(p)) => host.packet(&p),
            Ok(None) => {}
            Err(_) => return end(&shared, &mut sink, &inputs),
        }
        while let Ok((c, reply)) = rx.try_recv() {
            let _ = reply.send(host.command(c));
        }
        host.tick(start.elapsed().as_millis() as u64);
        let ups = host.take_up();
        let changed = !ups.is_empty();
        for u in ups {
            handle(u, &host, &bonds_file, sink.as_mut(), &mut inputs, &shared);
        }
        if changed {
            publish(&host, &inputs, &shared);
        }
    }
}

fn handle(
    u: Up,
    host: &Host,
    file: &Path,
    sink: &mut dyn InputSink,
    inputs: &mut Vec<u32>,
    shared: &Mutex<Shared>,
) {
    let Ok(mut s) = shared.lock() else { return };
    match u {
        Up::Ready(ok) => s.ready = ok,
        Up::BondsChanged => {
            let _ = host.bonds.save(file);
        }
        Up::Ask {
            token,
            addr,
            name,
            ask,
        } => {
            if s.questions.len() < MAX_ASKS {
                let kind = host
                    .devices
                    .get(&addr)
                    .map(|d| d.kind())
                    .unwrap_or(Kind::Other);
                s.questions.push_back(Question {
                    token,
                    addr,
                    name,
                    kind,
                    ask,
                });
            }
        }
        Up::Paired { addr, reason } => {
            s.results.push_back((addr, reason));
            while s.results.len() > 8 {
                s.results.pop_front();
            }
        }
        Up::InputAdded { id, kind, name } => {
            sink.added(id, kind, &name);
            inputs.push(id);
        }
        Up::Input { id, events } => sink.events(id, &events),
        Up::InputRemoved { id } => {
            sink.removed(id);
            inputs.retain(|&i| i != id);
        }
        Up::Changed => {}
    }
}

fn publish(host: &Host, inputs: &[u32], shared: &Mutex<Shared>) {
    let mut list: Vec<DeviceView> = host
        .bonds
        .list
        .iter()
        .rev()
        .map(|b| {
            let d = host.devices.get(&b.addr);
            DeviceView {
                addr: b.addr,
                name: b.name.clone(),
                kind: b.kind,
                rssi: d.and_then(|d| d.rssi),
                le: b.le,
                paired: true,
                connected: host.connected(&b.addr),
                input: host.connected(&b.addr) && !inputs.is_empty() && host.input_from(&b.addr),
            }
        })
        .collect();
    for d in host.devices.values() {
        if host.bonds.find(&d.addr).is_none() {
            list.push(DeviceView {
                addr: d.addr,
                name: d.name.clone(),
                kind: d.kind(),
                rssi: d.rssi,
                le: d.le,
                paired: false,
                connected: host.connected(&d.addr),
                input: false,
            });
        }
    }
    if let Ok(mut s) = shared.lock() {
        s.devices = list;
        s.scanning = host.is_scanning();
        s.ready = host.is_ready();
    }
}

fn end(shared: &Mutex<Shared>, sink: &mut Box<dyn InputSink>, inputs: &[u32]) {
    for &id in inputs {
        sink.removed(id);
    }
    if let Ok(mut s) = shared.lock() {
        s.ready = false;
        s.ended = true;
    }
}

/// Input: one uinput device per Bluetooth keyboard / mouse.
pub struct Uinput {
    root: PathBuf,
    devs: Vec<(u32, ultar_hal_linux::uinput::VirtualInput)>,
}

impl Uinput {
    /// Devices are created below `root` (`dev/uinput` on Linux, `dev/ultar/uinput` on
    /// the Ultar kernel).
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            devs: Vec::new(),
        }
    }

    /// Number of devices created.
    pub fn len(&self) -> usize {
        self.devs.len()
    }

    /// No device created.
    pub fn is_empty(&self) -> bool {
        self.devs.is_empty()
    }
}

impl InputSink for Uinput {
    fn added(&mut self, id: u32, kind: InputKind, name: &str) {
        use ultar_hal_linux::uinput::{Shape, VirtualInput};
        let shape = if kind == InputKind::Keyboard {
            Shape::Keyboard
        } else {
            Shape::Mouse
        };
        let Some(node) = ultar_hal_linux::uinput::node(&self.root) else {
            return;
        };
        if let Ok(d) = VirtualInput::create_at(&node, shape, name) {
            self.devs.push((id, d));
        }
    }
    fn events(&mut self, id: u32, events: &[InputEvent]) {
        if let Some((_, d)) = self.devs.iter_mut().find(|(i, _)| *i == id) {
            let ev: Vec<(u16, u16, i32)> =
                events.iter().map(|e| (e.kind, e.code, e.value)).collect();
            let _ = d.send(&ev);
        }
    }
    fn removed(&mut self, id: u32) {
        // Dropping the device destroys it; the kernel releases keys still held.
        self.devs.retain(|(i, _)| *i != id);
    }
}
