//! The engine: one Servo, one software rendering context, one web view per browser tab. The
//! loop waits on the browser socket and on Servo's wake-ups, applies commands, lets Servo work,
//! sends what happened and paints the visible tab when the browser has read the last frame.

use crate::delegate::Delegate;
use crate::frames::FrameBuffer;
use crate::link::{Link, LinkError};
use crate::waker::Wakeups;
use dpi::PhysicalSize;
use euclid::Scale;
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use servo::{
    DeviceIntRect, DeviceIntSize, RenderingContext, Servo, ServoBuilder, SoftwareRenderingContext,
    UserContentManager, UserScript, WebView, WebViewBuilder,
};
use std::collections::BTreeMap;
use std::rc::Rc;
use ultar_web::engine::{EngineEvent, TabId, MAX_TABS};
use ultar_web::ipc::{Command, HostEvent, PROTOCOL_VERSION};
use ultar_web::privacy::persona::Persona;

/// Size of the drawing surface before the browser names one.
const FIRST_SIZE: (u32, u32) = (1400, 900);
/// Wait between frames while a page animates (about 60 per second).
const FRAME_WAIT: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 16_000_000,
};

/// Why the host stopped.
#[derive(Debug)]
pub enum HostError {
    /// The link to the browser failed.
    Link(LinkError),
    /// The drawing surface could not be made.
    Rendering(String),
    /// The system refused something (pipe, shared memory).
    Io(std::io::Error),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Link(e) => write!(f, "{e}"),
            HostError::Rendering(e) => write!(f, "rendering: {e}"),
            HostError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<LinkError> for HostError {
    fn from(e: LinkError) -> Self {
        HostError::Link(e)
    }
}

impl From<std::io::Error> for HostError {
    fn from(e: std::io::Error) -> Self {
        HostError::Io(e)
    }
}

struct Tab {
    /// Made with the first address ([`Host::load`]).
    view: Option<WebView>,
    size: (u32, u32),
    scale: f32,
    buffer: Option<FrameBuffer>,
    /// A frame went out and the browser has not read it yet.
    waiting: bool,
}

/// The running engine.
pub struct Host {
    servo: Servo,
    rendering: Rc<SoftwareRenderingContext>,
    delegate: Rc<Delegate>,
    content: Rc<UserContentManager>,
    tabs: BTreeMap<TabId, Tab>,
    visible: Option<TabId>,
    link: Link,
    wakeups: Wakeups,
}

impl Host {
    /// Starts Servo for `persona`, everything going out through `proxy`.
    pub fn new(link: Link, persona: Persona, proxy: &str) -> Result<Host, HostError> {
        let wakeups = Wakeups::new()?;
        let rendering = Rc::new(
            SoftwareRenderingContext::new(PhysicalSize::new(FIRST_SIZE.0, FIRST_SIZE.1))
                .map_err(|e| HostError::Rendering(format!("{e:?}")))?,
        );
        let servo = ServoBuilder::default()
            .opts(crate::persona::opts())
            .preferences(crate::persona::preferences(&persona, proxy))
            .event_loop_waker(wakeups.waker())
            .build();
        let content = Rc::new(UserContentManager::new(&servo));
        content.add_script(Rc::new(UserScript::new(
            crate::persona::script(&persona),
            None,
        )));
        let shared: Rc<dyn RenderingContext> = rendering.clone();
        Ok(Host {
            delegate: Delegate::new(persona, shared),
            servo,
            rendering,
            content,
            tabs: BTreeMap::new(),
            visible: None,
            link,
            wakeups,
        })
    }

    /// Serves the browser until it says `Quit` or goes away.
    pub fn run(&mut self) -> Result<(), HostError> {
        self.link
            .send(&HostEvent::Ready(PROTOCOL_VERSION).encode())?;
        loop {
            self.wait()?;
            self.wakeups.drain();
            for command in self.link.receive()? {
                if matches!(command, Command::Quit) {
                    return Ok(());
                }
                self.apply(command);
            }
            self.servo.spin_event_loop();
            self.send_events()?;
            self.paint()?;
        }
    }

    fn wait(&self) -> Result<(), HostError> {
        let animating = {
            let state = self.delegate.state.borrow();
            self.visible.is_some_and(|t| state.animating.contains(&t))
        };
        let (link, wakeups) = (self.link.fd(), self.wakeups.fd());
        let mut fds = [
            PollFd::new(&link, PollFlags::IN),
            PollFd::new(&wakeups, PollFlags::IN),
        ];
        match poll(&mut fds, animating.then_some(&FRAME_WAIT)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(e) => Err(HostError::Io(e.into())),
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::CreateTab(tab, _private) => self.create_tab(tab),
            Command::CloseTab(tab) => self.close_tab(tab),
            Command::Visible(tab, visible) => self.show(tab, visible),
            Command::Resize(tab, w, h, scale) => self.resize(tab, w, h, scale),
            Command::FrameDone(tab) => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.waiting = false;
                }
            }
            Command::Permission(request, allow) => self.delegate.answer(request, allow),
            // no content blocker and no download manager in this engine yet
            Command::Protection(..) | Command::CancelDownload(_) | Command::Quit => {}
            other => self.page_command(other),
        }
    }

    /// Commands that act on the page of a tab.
    fn page_command(&mut self, command: Command) {
        if let Command::Load(tab, address) = &command {
            if let Ok(url) = url::Url::parse(address) {
                self.load(*tab, url);
            }
            return;
        }
        let view = |tab: &TabId| self.tabs.get(tab).and_then(|t| t.view.clone());
        match command {
            Command::Back(tab) => {
                if let Some(v) = view(&tab).filter(WebView::can_go_back) {
                    v.go_back(1);
                }
            }
            Command::Forward(tab) => {
                if let Some(v) = view(&tab).filter(WebView::can_go_forward) {
                    v.go_forward(1);
                }
            }
            Command::Reload(tab) => view(&tab).iter().for_each(WebView::reload),
            Command::Stop(tab) => {
                if let Some(v) = view(&tab) {
                    v.evaluate_javascript("window.stop()", |_| {});
                }
            }
            Command::Pointer(tab, input) => {
                if let Some(v) = view(&tab) {
                    v.notify_input_event(crate::input::pointer(input));
                }
            }
            Command::Key(tab, input) => {
                if let Some(v) = view(&tab) {
                    v.notify_input_event(crate::input::key(input));
                }
            }
            _ => {}
        }
    }

    /// A tab gets its web view with its first address (a view built without one would load
    /// `about:blank` first, and an address given before that page exists is lost).
    fn create_tab(&mut self, tab: TabId) {
        if self.tabs.contains_key(&tab) || self.tabs.len() >= MAX_TABS {
            return;
        }
        self.delegate
            .state
            .borrow_mut()
            .views
            .insert(tab, (FIRST_SIZE, 1.0));
        self.tabs.insert(
            tab,
            Tab {
                view: None,
                size: FIRST_SIZE,
                scale: 1.0,
                buffer: None,
                waiting: false,
            },
        );
    }

    fn load(&mut self, tab: TabId, url: url::Url) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        if let Some(view) = &t.view {
            view.load(url);
            return;
        }
        let rendering: Rc<dyn RenderingContext> = self.rendering.clone();
        let delegate: Rc<dyn servo::WebViewDelegate> = self.delegate.clone();
        let view = WebViewBuilder::new(&self.servo, rendering)
            .delegate(delegate)
            .user_content_manager(self.content.clone())
            .hidpi_scale_factor(Scale::new(t.scale))
            .url(url)
            .build();
        view.resize(PhysicalSize::new(t.size.0, t.size.1));
        if self.visible == Some(tab) {
            view.show();
            view.set_focused(true);
        } else {
            view.hide();
        }
        self.delegate.state.borrow_mut().tabs.insert(view.id(), tab);
        t.view = Some(view);
    }

    fn close_tab(&mut self, tab: TabId) {
        if let Some(t) = self.tabs.remove(&tab) {
            let mut state = self.delegate.state.borrow_mut();
            if let Some(view) = &t.view {
                state.tabs.remove(&view.id());
            }
            state.views.remove(&tab);
            state.dirty.remove(&tab);
            state.animating.remove(&tab);
        }
        if self.visible == Some(tab) {
            self.visible = None;
        }
    }

    fn show(&mut self, tab: TabId, visible: bool) {
        let Some(t) = self.tabs.get(&tab) else { return };
        if !visible {
            t.view.iter().for_each(WebView::hide);
            if self.visible == Some(tab) {
                self.visible = None;
            }
            return;
        }
        if let Some(old) = self
            .visible
            .filter(|v| *v != tab)
            .and_then(|v| self.tabs.get(&v))
        {
            old.view.iter().for_each(WebView::hide);
        }
        let (w, h) = t.size;
        self.rendering.resize(PhysicalSize::new(w, h));
        if let Some(view) = &t.view {
            view.show();
            view.set_focused(true);
        }
        self.visible = Some(tab);
        self.delegate.state.borrow_mut().dirty.insert(tab);
    }

    fn resize(&mut self, tab: TabId, w: u32, h: u32, scale: f32) {
        // 0 × 0: a hidden window keeps its page as it was
        let Some(t) = self.tabs.get_mut(&tab).filter(|_| w > 0 && h > 0) else {
            return;
        };
        t.size = (w, h);
        t.scale = scale;
        if let Some(view) = &t.view {
            view.set_hidpi_scale_factor(Scale::new(scale));
            view.resize(PhysicalSize::new(w, h));
        }
        let mut state = self.delegate.state.borrow_mut();
        state.views.insert(tab, ((w, h), scale));
        state.dirty.insert(tab);
        drop(state);
        if self.visible == Some(tab) {
            self.rendering.resize(PhysicalSize::new(w, h));
        }
    }

    fn send_events(&mut self) -> Result<(), HostError> {
        for event in self.delegate.take_events() {
            self.link.send(&HostEvent::Engine(event).encode())?;
        }
        Ok(())
    }

    /// Paints the visible tab if it has a new frame and the browser read the last one.
    fn paint(&mut self) -> Result<(), HostError> {
        let Some(tab) = self.visible else {
            return Ok(());
        };
        let Some(t) = self.tabs.get_mut(&tab).filter(|t| !t.waiting) else {
            return Ok(());
        };
        let Some(view) = t.view.as_ref() else {
            return Ok(());
        };
        if !self.delegate.state.borrow_mut().dirty.remove(&tab) {
            return Ok(());
        }
        // read back before `present`: the swap leaves the new back buffer undefined
        self.rendering
            .make_current()
            .map_err(|e| HostError::Rendering(format!("{e:?}")))?;
        view.paint();
        let size = self.rendering.size();
        let rect =
            DeviceIntRect::from_size(DeviceIntSize::new(size.width as i32, size.height as i32));
        let picture = self.rendering.read_to_image(rect);
        self.rendering.present();
        let Some(picture) = picture else {
            return Ok(());
        };
        let (w, h) = (picture.width(), picture.height());
        let fresh = t
            .buffer
            .as_ref()
            .is_none_or(|b| (b.width(), b.height()) != (w, h));
        if fresh {
            let buffer = FrameBuffer::new(w, h)?;
            let message = HostEvent::Buffer(tab, w, h, buffer.stride()).encode();
            self.link.send_with_fd(&message, buffer.fd())?;
            t.buffer = Some(buffer);
        }
        let Some(buffer) = t.buffer.as_mut() else {
            return Ok(());
        };
        if let Some(damage) = buffer.update(picture.as_raw())? {
            t.waiting = true;
            self.link
                .send(&HostEvent::Engine(EngineEvent::Frame { tab, damage }).encode())?;
        }
        Ok(())
    }
}
