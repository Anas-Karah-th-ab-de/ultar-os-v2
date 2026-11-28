//! The bus side of the shared clipboard: the bridge to the compositor and the notice.

use crate::clip_bridge::{Bridge, Copied};
use crate::node::Effect;
use crate::service::MeshService;
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::notify::{Body, Category, Post, Urgency};
use ultar_bus::{Text, TextArg};
use ultar_svckit::serve::Done;
use ultar_svckit::Bus;

/// Application id of the notice.
pub const APP_ID: &str = "org.ultar.mesh";
/// Notice title (`{arg0}` is the device's name).
pub const NOTICE_TITLE: &str = "mesh.clip.title";
/// Notice body.
pub const NOTICE_BODY: &str = "mesh.clip.body";
const CALL_TIME: Duration = Duration::from_secs(5);

/// What the service keeps of the clipboard bridge.
#[derive(Default)]
pub(crate) struct ClipHost {
    pub(crate) socket: Option<PathBuf>,
    pub(crate) bridge: Option<Bridge>,
}

/// The notice that an entry came from `from_name`.
pub fn notice(from_name: &str) -> Post {
    Post {
        urgency: Urgency::Low,
        category: Category::System,
        app_id: APP_ID.to_owned(),
        title: Text::key(NOTICE_TITLE).with(TextArg::Verbatim(from_name.to_owned())),
        body: Body::Localized(Text::key(NOTICE_BODY)),
        actions: Vec::new(),
    }
}

impl MeshService {
    /// Shares the clipboard through the compositor at `socket` (without it the clipboard
    /// stays on this device).
    #[must_use]
    pub fn with_compositor(mut self, socket: PathBuf) -> Self {
        self.clip.socket = Some(socket);
        self
    }

    /// Connects to the compositor when one is named (only while the mesh is on: with the
    /// mesh off nothing reads the clipboard).
    pub(crate) fn start_clip(&mut self, bus: &Bus<'_, Self>) {
        let Some(socket) = self.clip.socket.clone() else {
            return;
        };
        if self.clip.bridge.is_some() {
            return;
        }
        let poster = bus.poster();
        self.clip.bridge = Bridge::start(socket, move |copied| {
            let work: Done<Self> = Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                service.copied_here(copied, bus);
            });
            poster.post(work)
        });
    }

    /// The person copied something on this device.
    fn copied_here(&mut self, copied: Copied, bus: &Bus<'_, Self>) {
        let effects = match copied {
            Copied::Text(text) => self.runtime.node.clipboard_set(&text, false),
            Copied::Image(png) => self.runtime.node.clipboard_image_set(&png),
        };
        self.act(effects, bus);
    }

    /// What the clipboard of the node wants of the host; `None` when it was done here.
    pub(crate) fn clip_effect(&mut self, effect: Effect, bus: &Bus<'_, Self>) -> Option<Effect> {
        match effect {
            Effect::ClipboardImage { png, .. } => {
                if let Some(bridge) = &self.clip.bridge {
                    bridge.offer_image(png);
                }
            }
            Effect::ClipboardClear => {
                if let Some(bridge) = &self.clip.bridge {
                    bridge.clear();
                }
            }
            Effect::ClipNotice { from_name } => {
                let post = notice(&from_name);
                let _ = bus.later(move |client| {
                    let _ = client.call(&post, CALL_TIME);
                    let done: Done<Self> = Box::new(|_: &mut Self, _: &Bus<'_, Self>| {});
                    done
                });
            }
            other => return Some(other),
        }
        None
    }

    /// Text that came from another device is offered as the local selection.
    pub(crate) fn clip_text_arrived(&mut self, text: String) {
        if let Some(bridge) = &self.clip.bridge {
            bridge.offer_text(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_names_the_device_and_is_quiet() {
        let post = notice("Desk");
        assert_eq!(post.title.key, NOTICE_TITLE);
        assert_eq!(post.title.args, vec![TextArg::Verbatim("Desk".into())]);
        assert!(matches!(&post.body, Body::Localized(t) if t.key == NOTICE_BODY));
        assert_eq!(post.urgency, Urgency::Low);
        assert!(post.actions.is_empty());
    }
}
