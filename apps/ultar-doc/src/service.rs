//! The app's bus ports. The only service the word processor needs is the device mesh: it
//! reports what is open so the person can continue on another device
//! (`docs/integration/ultar-mesh.md`, "B4"). Said again only when the place changed; without
//! the mesh (no `--bus`) nothing happens.

use std::time::Duration;
use ultar_bus::Client;
use ultar_bus::messages::mesh::{Activity, SetActivity};

/// Timeout of a report; the mesh may be busy or absent and that must never block the UI.
const TIMEOUT: Duration = Duration::from_millis(300);

/// Tells the device mesh which document is open.
pub struct Mesh {
    bus: Client,
    reported: Option<String>,
}

impl Mesh {
    pub fn new(bus: Client) -> Mesh {
        Mesh { bus, reported: None }
    }

    /// Reports the open document; `uri` carries the place (`files://…#b<block>`), `title` the
    /// window title (data), `path` the absolute file.
    pub fn report(&mut self, app_id: &str, title: &str, uri: Option<&str>, path: Option<&str>) {
        if uri.is_none() && path.is_none() {
            return;
        }
        let key = uri.map_or_else(|| String::new(), |u| u.to_owned());
        if self.reported.as_deref() == Some(key.as_str()) {
            return;
        }
        let activity = Activity {
            app: app_id.to_owned(),
            title: title.to_owned(),
            uri: uri.map(str::to_owned),
            file: path.map(str::to_owned),
        };
        let _ = self.bus.call(&SetActivity { activity: Some(activity) }, TIMEOUT);
        self.reported = Some(key);
    }
}