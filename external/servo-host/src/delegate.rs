//! What Servo reports about its web views, turned into the browser's engine events. Servo calls
//! in on the host's own thread while it spins; the host collects the events after each spin.

use servo::{
    CreateNewWebViewRequest, LoadStatus, NavigationRequest, PermissionFeature, PermissionRequest,
    RenderingContext, ScreenGeometry, WebView, WebViewBuilder, WebViewDelegate, WebViewId,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::{Rc, Weak};
use ultar_web::engine::{EngineEvent, PermissionKind, RequestId, TabId};
use ultar_web::privacy::persona::Persona;
use url::Url;

/// Pages a page may open in the window (everything else of Servo's stays inside the engine).
fn may_show(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "blob") || url.as_str() == "about:blank"
}

/// What the host and the delegate share.
#[derive(Default)]
pub struct State {
    /// Web views of the browser's tabs.
    pub tabs: HashMap<WebViewId, TabId>,
    /// Size and scale of each tab (for the screen pages are told about).
    pub views: HashMap<TabId, ((u32, u32), f32)>,
    /// Events for the browser, in order.
    pub events: Vec<EngineEvent>,
    /// Tabs with a new frame to paint.
    pub dirty: BTreeSet<TabId>,
    /// Tabs that animate (the loop then spins at frame rate).
    pub animating: BTreeSet<TabId>,
    permissions: BTreeMap<u32, PermissionRequest>,
    next_request: u32,
    /// Windows a page opened, until their address is known (they become browser tabs).
    popups: Vec<(TabId, WebView)>,
}

/// The delegate of every web view of the host.
pub struct Delegate {
    me: Weak<Delegate>,
    persona: Persona,
    rendering: Rc<dyn RenderingContext>,
    /// The shared state.
    pub state: RefCell<State>,
}

impl Delegate {
    /// A delegate whose pop-up windows render into `rendering`.
    pub fn new(persona: Persona, rendering: Rc<dyn RenderingContext>) -> Rc<Delegate> {
        Rc::new_cyclic(|me| Delegate {
            me: me.clone(),
            persona,
            rendering,
            state: RefCell::new(State::default()),
        })
    }

    /// The events collected since the last call.
    pub fn take_events(&self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.state.borrow_mut().events)
    }

    /// Answers a waiting permission request (unknown ids are ignored: already answered).
    pub fn answer(&self, request: RequestId, allow: bool) {
        let pending = self.state.borrow_mut().permissions.remove(&request.0);
        match pending {
            Some(p) if allow => p.allow(),
            Some(p) => p.deny(),
            None => {}
        }
    }

    fn tab(&self, view: &WebView) -> Option<TabId> {
        self.state.borrow().tabs.get(&view.id()).copied()
    }

    fn push(&self, event: EngineEvent) {
        self.state.borrow_mut().events.push(event);
    }

    fn navigated(&self, tab: TabId, view: &WebView, url: &Url) {
        self.push(EngineEvent::Navigated {
            tab,
            url: url.to_string(),
            can_back: view.can_go_back(),
            can_forward: view.can_go_forward(),
        });
        self.push(EngineEvent::Security {
            tab,
            secure: url.scheme() == "https",
        });
    }

    /// A pop-up got its address: the browser opens it as a tab, the pop-up view goes.
    fn adopt_popup(&self, view: &WebView, url: &Url) -> bool {
        let mut state = self.state.borrow_mut();
        let Some(i) = state.popups.iter().position(|(_, v)| v.id() == view.id()) else {
            return false;
        };
        if url.as_str() == "about:blank" {
            return true;
        }
        let (opener, popup) = state.popups.remove(i);
        if matches!(url.scheme(), "http" | "https") {
            state.events.push(EngineEvent::NewTab {
                opener,
                url: url.to_string(),
            });
        }
        // the view is let go only after the state, Servo may call back while it closes
        drop(state);
        drop(popup);
        true
    }
}

fn permission_kind(feature: PermissionFeature) -> Option<PermissionKind> {
    match feature {
        PermissionFeature::Geolocation => Some(PermissionKind::Location),
        PermissionFeature::Notifications => Some(PermissionKind::Notifications),
        PermissionFeature::Camera => Some(PermissionKind::Camera),
        PermissionFeature::Microphone => Some(PermissionKind::Microphone),
        _ => None,
    }
}

impl WebViewDelegate for Delegate {
    fn screen_geometry(&self, view: WebView) -> Option<ScreenGeometry> {
        let tab = self.tab(&view)?;
        let (size, scale) = *self.state.borrow().views.get(&tab)?;
        Some(crate::persona::screen(&self.persona, scale, size))
    }

    fn notify_url_changed(&self, view: WebView, url: Url) {
        if self.adopt_popup(&view, &url) {
            return;
        }
        if let Some(tab) = self.tab(&view) {
            self.navigated(tab, &view, &url);
        }
    }

    fn notify_history_changed(&self, view: WebView, entries: Vec<Url>, current: usize) {
        let (Some(tab), Some(url)) = (self.tab(&view), entries.get(current)) else {
            return;
        };
        self.push(EngineEvent::Navigated {
            tab,
            url: url.to_string(),
            can_back: current > 0,
            can_forward: current + 1 < entries.len(),
        });
    }

    fn notify_page_title_changed(&self, view: WebView, title: Option<String>) {
        if let Some(tab) = self.tab(&view) {
            self.push(EngineEvent::Title {
                tab,
                title: title.unwrap_or_default(),
            });
        }
    }

    fn notify_load_status_changed(&self, view: WebView, status: LoadStatus) {
        let Some(tab) = self.tab(&view) else { return };
        let (loading, progress) = match status {
            LoadStatus::Started => (true, 0.1),
            LoadStatus::HeadParsed => (true, 0.6),
            _ => (false, 1.0),
        };
        self.push(EngineEvent::Progress {
            tab,
            loading,
            progress,
        });
    }

    fn notify_new_frame_ready(&self, view: WebView) {
        if let Some(tab) = self.tab(&view) {
            self.state.borrow_mut().dirty.insert(tab);
        }
    }

    fn notify_animating_changed(&self, view: WebView, animating: bool) {
        let Some(tab) = self.tab(&view) else { return };
        let mut state = self.state.borrow_mut();
        if animating {
            state.animating.insert(tab);
        } else {
            state.animating.remove(&tab);
        }
    }

    fn notify_crashed(&self, view: WebView, reason: String, _backtrace: Option<String>) {
        eprintln!("ultar-web-engine: page crashed: {reason}");
        if let Some(tab) = self.tab(&view) {
            self.push(EngineEvent::Crashed { tab });
        }
    }

    fn request_navigation(&self, view: WebView, request: NavigationRequest) {
        if self.adopt_popup(&view, &request.url) {
            if request.url.as_str() == "about:blank" {
                request.allow();
            } else {
                request.deny();
            }
            return;
        }
        if may_show(&request.url) {
            request.allow();
        } else {
            request.deny();
        }
    }

    fn request_create_new(&self, parent: WebView, request: CreateNewWebViewRequest) {
        let (Some(opener), Some(me)) = (self.tab(&parent), self.me.upgrade()) else {
            return;
        };
        let delegate: Rc<dyn WebViewDelegate> = me;
        let builder: WebViewBuilder = request.builder(self.rendering.clone());
        let view = builder.delegate(delegate).build();
        view.hide();
        self.state.borrow_mut().popups.push((opener, view));
    }

    fn request_permission(&self, view: WebView, request: PermissionRequest) {
        let (Some(tab), Some(kind)) = (self.tab(&view), permission_kind(request.feature())) else {
            request.deny();
            return;
        };
        let host = view
            .url()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        let mut state = self.state.borrow_mut();
        state.next_request = state.next_request.wrapping_add(1);
        let id = state.next_request;
        state.permissions.insert(id, request);
        state.events.push(EngineEvent::Permission {
            tab,
            request: RequestId(id),
            host,
            kind,
        });
    }
}
