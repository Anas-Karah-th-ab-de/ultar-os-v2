//! The window of the remote view: choose a device, wait for its person to say yes, then the
//! other screen with a toolbar (stop, full screen, picture quality, send keys). Pointer and
//! keys over the picture go to the other device. When the session ends the window says why
//! (refused, stopped, locked, connection lost) and offers to try again.

use crate::keys::{self, Combo};
use crate::picture::{self, Quality, Remote};
use crate::port::{Port, PortEvent};
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::Peer;
use ultar_bus::messages::mesh_view::{Button as Pad, EndReason, RemoteInput, ViewEvent};
use ultar_ui::canvas::Canvas;
use ultar_ui::event::{Action, ActionKind, Event, Key, PointerButton};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, EmptyState, Label, PixelArea, Select, Text};

/// How long the window waits for the other person to say yes before it gives up.
pub const WAIT_LIMIT: Duration = Duration::from_secs(90);

/// What the window shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Pick a device.
    Choose,
    /// Asked; the other person has not answered.
    Waiting,
    /// The other screen is on.
    Running,
    /// The other device did not accept.
    Refused,
    /// The session is over for this reason (stopped, lost, locked).
    Ended(EndReason),
}

/// Something the window wants from the platform that only the program around it can do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wish {
    /// Go to (`true`) or leave (`false`) full screen.
    Fullscreen(bool),
}

/// The pointer as the window tracks it towards the other device.
#[derive(Default)]
struct Mouse {
    /// The last position not sent yet (moves are held back to one per look).
    pending: Option<(u32, u32)>,
    /// The button held down.
    held: Option<Pad>,
}

#[derive(Default)]
struct Nodes {
    devices: Option<NodeId>,
    connect: Option<NodeId>,
    cancel: Option<NodeId>,
    stop: Option<NodeId>,
    full: Option<NodeId>,
    quality: Option<NodeId>,
    keys: Option<NodeId>,
    retry: Option<NodeId>,
    back: Option<NodeId>,
    picture: Option<NodeId>,
}

/// The remote view window.
pub struct RemoteApp {
    /// The widget tree (the window draws it).
    pub ui: Ui,
    port: Box<dyn Port>,
    phase: Phase,
    peers: Vec<Peer>,
    choice: usize,
    target: Option<(String, String)>,
    remote: Option<Remote>,
    quality: Quality,
    fullscreen: bool,
    wish: Option<Wish>,
    nodes: Nodes,
    mouse: Mouse,
    waiting_since: Option<Instant>,
    dirty: bool,
    /// Input events the mesh did not take (the link is slow; they are not queued).
    pub dropped: u64,
}

fn pad(button: PointerButton) -> Pad {
    match button {
        PointerButton::Primary => Pad::Left,    // physical
        PointerButton::Secondary => Pad::Right, // physical
        PointerButton::Middle => Pad::Middle,
    }
}

/// What the builders need to know besides the phase.
struct Look {
    name: String,
    choice: usize,
    quality: Quality,
    fullscreen: bool,
}

/// A bar of buttons under (or above) the content.
fn bar(ui: &mut Ui) -> Result<NodeId, UiError> {
    let root = ui.root();
    ui.add(
        root,
        Flex::row()
            .gap(Space::S8)
            .padding(Padding::all(Space::S8))
            .cross(CrossAlign::Center),
    )
}

fn build_choose(ui: &mut Ui, n: &mut Nodes, names: &[String], look: &Look) -> Result<(), UiError> {
    let root = ui.root();
    if names.is_empty() {
        let empty = EmptyState::new(IconId::Monitor, Label::key("remote.none.title"))
            .message(Label::key("remote.none.message"));
        ui.add_sized(root, empty, Sizing::grow(1))?;
        return Ok(());
    }
    let head = EmptyState::new(IconId::Monitor, Label::key("remote.window"));
    ui.add_sized(root, head, Sizing::grow(1))?;
    let row = bar(ui)?;
    let options = names.iter().map(|x| Label::data(x)).collect();
    let select = Select::new(Label::key("remote.device"), options);
    let select = ui.add_sized(row, select, Sizing::grow(1))?;
    ui.set_selected(select, look.choice)?;
    n.devices = Some(select);
    let connect = Button::new(Label::key("remote.connect")).variant(ButtonVariant::Primary);
    n.connect = Some(ui.add(row, connect)?);
    Ok(())
}

fn build_waiting(ui: &mut Ui, n: &mut Nodes, look: &Look) -> Result<(), UiError> {
    let root = ui.root();
    let title = Label::key_with("remote.waiting.title", &[("name", &look.name)]);
    let wait =
        EmptyState::new(IconId::ScreenShare, title).message(Label::key("remote.waiting.message"));
    ui.add_sized(root, wait, Sizing::grow(1))?;
    let row = bar(ui)?;
    n.cancel = Some(ui.add(row, Button::new(Label::key("remote.cancel")))?);
    Ok(())
}

fn build_running(ui: &mut Ui, n: &mut Nodes, look: &Look) -> Result<(), UiError> {
    let root = ui.root();
    let row = bar(ui)?;
    let title = Text::new(Label::key_with("remote.running", &[("name", &look.name)]));
    ui.add_sized(row, title, Sizing::grow(1))?;
    let mut keys = vec![Label::key("remote.keys.none")];
    keys.extend(Combo::ALL.iter().map(|c| Label::key(c.key())));
    n.keys = Some(ui.add(row, Select::new(Label::key("remote.keys"), keys))?);
    let qualities = vec![
        Label::key("remote.quality.smooth"),
        Label::key("remote.quality.sharp"),
    ];
    let quality = ui.add(row, Select::new(Label::key("remote.quality"), qualities))?;
    ui.set_selected(quality, usize::from(look.quality == Quality::Sharp))?;
    n.quality = Some(quality);
    let full = if look.fullscreen {
        "remote.leave_fullscreen"
    } else {
        "remote.fullscreen"
    };
    n.full = Some(ui.add(row, Button::new(Label::key(full)))?);
    let stop = Button::new(Label::key("remote.stop")).variant(ButtonVariant::Danger);
    n.stop = Some(ui.add(row, stop)?);
    let area = PixelArea::new(Label::key("remote.picture"));
    n.picture = Some(ui.add_sized(root, area, Sizing::grow(1))?);
    Ok(())
}

fn build_ended(ui: &mut Ui, n: &mut Nodes, phase: Phase, look: &Look) -> Result<(), UiError> {
    let root = ui.root();
    let (icon, title, message) = match phase {
        Phase::Ended(EndReason::Locked) => {
            (IconId::Lock, "remote.locked.title", "remote.locked.message")
        }
        Phase::Ended(EndReason::Lost) => {
            (IconId::Warning, "remote.lost.title", "remote.lost.message")
        }
        Phase::Ended(_) => (
            IconId::Monitor,
            "remote.stopped.title",
            "remote.stopped.message",
        ),
        _ => (
            IconId::Warning,
            "remote.refused.title",
            "remote.refused.message",
        ),
    };
    let args = [("name", look.name.as_str())];
    let end = EmptyState::new(icon, Label::key_with(title, &args))
        .message(Label::key_with(message, &args));
    ui.add_sized(root, end, Sizing::grow(1))?;
    let row = bar(ui)?;
    let retry = Button::new(Label::key("remote.retry")).variant(ButtonVariant::Primary);
    n.retry = Some(ui.add(row, retry)?);
    n.back = Some(ui.add(row, Button::new(Label::key("remote.back")))?);
    Ok(())
}

impl RemoteApp {
    /// A window over `ui` that talks to the mesh through `port`.
    pub fn new(ui: Ui, port: Box<dyn Port>) -> Result<RemoteApp, UiError> {
        let mut app = RemoteApp {
            ui,
            port,
            phase: Phase::Choose,
            peers: Vec::new(),
            choice: 0,
            target: None,
            remote: None,
            quality: Quality::Smooth,
            fullscreen: false,
            wish: None,
            nodes: Nodes::default(),
            mouse: Mouse::default(),
            waiting_since: None,
            dirty: false,
            dropped: 0,
        };
        app.rebuild()?;
        Ok(app)
    }

    /// What the window shows now.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The devices that can be looked at (paired and reachable), in the mesh's order.
    pub fn candidates(&self) -> Vec<&Peer> {
        self.peers
            .iter()
            .filter(|p| p.paired && p.reachable)
            .collect()
    }

    /// The device of the session (id, name).
    pub fn target(&self) -> Option<(&str, &str)> {
        self.target.as_ref().map(|(i, n)| (i.as_str(), n.as_str()))
    }

    /// The remote screen as the window holds it (while a session runs).
    pub fn remote(&self) -> Option<&Remote> {
        self.remote.as_ref()
    }

    /// `true` while the window is in full screen.
    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// What the program around the window should do now, once.
    pub fn take_wish(&mut self) -> Option<Wish> {
        self.wish.take()
    }

    fn name(&self) -> String {
        self.target
            .as_ref()
            .map_or_else(String::new, |(_, n)| n.clone())
    }

    /// The window of the picture in the window's pixels: (x, y, width, height).
    fn area(&mut self) -> Option<(f32, f32, u32, u32)> {
        self.ui.layout();
        let rect = self.ui.rect(self.nodes.picture?)?;
        let s = self.ui.env().scale;
        let (w, h) = ((rect.w * s).round(), (rect.h * s).round());
        (w >= 1.0 && h >= 1.0).then_some((rect.x, rect.y, w as u32, h as u32))
    }

    /// Builds the tree for the phase (also after a size change).
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        self.nodes = Nodes::default();
        let phase = self.phase;
        let name = self.name();
        let names: Vec<String> = self.candidates().iter().map(|p| p.name.clone()).collect();
        self.choice = self.choice.min(names.len().saturating_sub(1));
        let look = Look {
            name,
            choice: self.choice,
            quality: self.quality,
            fullscreen: self.fullscreen,
        };
        let ui = &mut self.ui;
        ui.clear();
        ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
        match phase {
            Phase::Choose => build_choose(ui, &mut self.nodes, &names, &look)?,
            Phase::Waiting => build_waiting(ui, &mut self.nodes, &look)?,
            Phase::Running => build_running(ui, &mut self.nodes, &look)?,
            Phase::Refused | Phase::Ended(_) => build_ended(ui, &mut self.nodes, phase, &look)?,
        }
        self.dirty = true;
        self.redraw()
    }

    /// Paints the remote screen into the picture area.
    fn redraw(&mut self) -> Result<(), UiError> {
        self.dirty = false;
        let (Some(node), Some((_, _, w, h))) = (self.nodes.picture, self.area()) else {
            return Ok(());
        };
        let Some(remote) = self.remote.as_ref() else {
            return Ok(());
        };
        let data = picture::render(remote, (w, h), self.quality);
        let canvas = Canvas::from_bgrx(w, h, data).map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, canvas)
    }

    /// Starts a session with candidate `index`.
    pub fn connect(&mut self, index: usize, now: Instant) -> Result<(), UiError> {
        let Some(peer) = self
            .candidates()
            .get(index)
            .map(|p| (p.id.clone(), p.name.clone()))
        else {
            return Ok(());
        };
        self.target = Some(peer);
        self.begin(now)
    }

    fn begin(&mut self, now: Instant) -> Result<(), UiError> {
        let Some((id, _)) = self.target.clone() else {
            return Ok(());
        };
        self.remote = None;
        self.mouse.held = None;
        self.mouse.pending = None;
        if self.port.start(&id) {
            self.phase = Phase::Waiting;
            self.waiting_since = Some(now);
        } else {
            self.phase = Phase::Ended(EndReason::Lost);
        }
        self.rebuild()
    }

    /// The person ends the session (or cancels the wait) and goes back to the choice.
    pub fn stop(&mut self) -> Result<(), UiError> {
        if matches!(self.phase, Phase::Waiting | Phase::Running) {
            self.port.stop();
        }
        self.leave(Phase::Choose)
    }

    fn leave(&mut self, phase: Phase) -> Result<(), UiError> {
        self.phase = phase;
        self.remote = None;
        self.mouse.held = None;
        self.mouse.pending = None;
        self.waiting_since = None;
        if self.fullscreen {
            self.fullscreen = false;
            self.wish = Some(Wish::Fullscreen(false));
        }
        self.rebuild()
    }

    /// The actions of the widget tree.
    pub fn actions(&mut self, actions: Vec<Action>, now: Instant) -> Result<(), UiError> {
        for a in actions {
            let at = Some(a.node);
            match a.kind {
                ActionKind::Selected(i) if at == self.nodes.devices => self.choice = i,
                ActionKind::Selected(i) if at == self.nodes.quality => {
                    self.quality = if i == 1 {
                        Quality::Sharp
                    } else {
                        Quality::Smooth
                    };
                    self.rebuild()?;
                }
                ActionKind::Selected(i) if at == self.nodes.keys => {
                    if let Some(combo) = i.checked_sub(1).and_then(|c| Combo::ALL.get(c)) {
                        for input in combo.inputs() {
                            self.send(input);
                        }
                    }
                    // back to the heading; this also takes the focus off the menu
                    self.rebuild()?;
                }
                ActionKind::Activated if at == self.nodes.connect => {
                    self.connect(self.choice, now)?;
                }
                ActionKind::Activated if at == self.nodes.cancel || at == self.nodes.stop => {
                    self.stop()?;
                }
                ActionKind::Activated if at == self.nodes.full => self.toggle_fullscreen()?,
                ActionKind::Activated if at == self.nodes.retry => self.begin(now)?,
                ActionKind::Activated if at == self.nodes.back => self.leave(Phase::Choose)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// Full screen on or off (F11 and the toolbar).
    pub fn toggle_fullscreen(&mut self) -> Result<(), UiError> {
        self.fullscreen = !self.fullscreen;
        self.wish = Some(Wish::Fullscreen(self.fullscreen));
        self.rebuild()
    }

    fn send(&mut self, input: RemoteInput) {
        self.flush_move();
        if self.phase == Phase::Running && !self.port.input(input) {
            self.dropped += 1;
        }
    }

    fn flush_move(&mut self) {
        if let Some((x, y)) = self.mouse.pending.take() {
            if self.phase == Phase::Running && !self.port.input(RemoteInput::Move { x, y }) {
                self.dropped += 1;
            }
        }
    }

    /// The remote position under window position `pos` (points); `clamp` keeps a drag that
    /// left the picture on its edge.
    fn remote_at(&mut self, pos: ultar_ui::Point, clamp: bool) -> Option<(u32, u32)> {
        let screen = self.remote.as_ref().map(|r| (r.width(), r.height()))?;
        let (x0, y0, w, h) = self.area()?;
        let s = self.ui.env().scale;
        let fit = picture::fit(screen, (w, h));
        let (px, py) = ((pos.x - x0) * s, (pos.y - y0) * s);
        if clamp {
            Some(fit.clamped(px, py))
        } else {
            fit.to_remote(px, py)
        }
    }

    /// A pointer event of the window: over the picture it goes to the other device.
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if self.phase != Phase::Running {
            return Ok(());
        }
        match event {
            Event::PointerMove { pos } => {
                if let Some(at) = self.remote_at(pos, self.mouse.held.is_some()) {
                    if self.mouse.pending != Some(at) {
                        self.mouse.pending = Some(at);
                    }
                }
            }
            Event::PointerDown { pos, button } => {
                if let Some((x, y)) = self.remote_at(pos, false) {
                    let button = pad(button);
                    self.mouse.held = Some(button);
                    self.send(RemoteInput::Button {
                        x,
                        y,
                        button,
                        down: true,
                    });
                }
            }
            Event::PointerUp { pos, button } => {
                if self.mouse.held == Some(pad(button)) {
                    if let Some((x, y)) = self.remote_at(pos, true) {
                        self.mouse.held = None;
                        self.send(RemoteInput::Button {
                            x,
                            y,
                            button: pad(button),
                            down: false,
                        });
                    }
                }
            }
            Event::Scroll { pos, lines } => {
                if let Some((x, y)) = self.remote_at(pos, false) {
                    let lines = lines.round().clamp(-100.0, 100.0) as i16;
                    if lines != 0 {
                        self.send(RemoteInput::Wheel { x, y, lines });
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A key the toolbar did not use: F11 is the window's own, every other key goes to the
    /// other device (keys without a code there, like Arabic letters, are dropped).
    pub fn key(&mut self, event: Event) -> Result<(), UiError> {
        let Event::Key { key, modifiers } = event else {
            return Ok(());
        };
        if key == Key::F(11) {
            return self.toggle_fullscreen();
        }
        if self.phase != Phase::Running {
            return Ok(());
        }
        if let Some(seq) = keys::sequence(key, modifiers) {
            for input in seq {
                self.send(input);
            }
        }
        Ok(())
    }

    /// News from the mesh, the held-back pointer move, the picture, the wait limit.
    pub fn poll(&mut self, now: Instant) -> Result<(), UiError> {
        for event in self.port.events() {
            match event {
                PortEvent::Peers(peers) => self.peers_changed(peers)?,
                PortEvent::View(e) => self.view_event(e)?,
                PortEvent::Lost => {
                    if matches!(self.phase, Phase::Waiting | Phase::Running) {
                        self.leave(Phase::Ended(EndReason::Lost))?;
                    }
                }
            }
        }
        if self.phase == Phase::Waiting
            && self
                .waiting_since
                .is_some_and(|t| now.duration_since(t) >= WAIT_LIMIT)
        {
            self.port.stop();
            self.leave(Phase::Refused)?;
        }
        self.flush_move();
        if self.dirty && self.phase == Phase::Running {
            self.redraw()?;
        }
        Ok(())
    }

    fn peers_changed(&mut self, peers: Vec<Peer>) -> Result<(), UiError> {
        let before: Vec<(String, String)> = self
            .candidates()
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        self.peers = peers;
        let after: Vec<(String, String)> = self
            .candidates()
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        if before != after && self.phase == Phase::Choose {
            return self.rebuild();
        }
        Ok(())
    }

    fn view_event(&mut self, event: ViewEvent) -> Result<(), UiError> {
        match (self.phase, event) {
            (Phase::Waiting, ViewEvent::Waiting { name }) => {
                if let Some(t) = self.target.as_mut() {
                    t.1 = name;
                }
                self.rebuild()
            }
            (Phase::Waiting, ViewEvent::Running { width, height }) => {
                match Remote::new(width, height) {
                    Some(r) => {
                        self.remote = Some(r);
                        self.phase = Phase::Running;
                        self.waiting_since = None;
                        self.rebuild()
                    }
                    None => {
                        self.port.stop();
                        self.leave(Phase::Ended(EndReason::Lost))
                    }
                }
            }
            (Phase::Running, ViewEvent::Picture { x, y, w, h, data }) => {
                if let Some(r) = self.remote.as_mut() {
                    self.dirty |= r.write(x, y, w, h, &data);
                }
                Ok(())
            }
            (Phase::Waiting | Phase::Running, ViewEvent::Ended { reason }) => {
                let phase = match reason {
                    EndReason::Refused => Phase::Refused,
                    other => Phase::Ended(other),
                };
                self.leave(phase)
            }
            _ => Ok(()),
        }
    }
}
