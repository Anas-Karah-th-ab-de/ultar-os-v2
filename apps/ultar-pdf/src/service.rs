//! The app's side of the device mesh: it reports which document is open on which page, so
//! the person can continue it on another device (`docs/integration/ultar-mesh.md`, "B4").
//! The bus policy allows every app exactly `mesh.set_activity`; nothing else is touched.
//!
//! The app id towards the bus and the compositor.

use std::time::Duration;
use ultar_bus::messages::mesh::{Activity, SetActivity};
use ultar_bus::Client;

/// App id towards compositor and bus (the launcher knows it as `pdf`).
pub const APP_ID: &str = "org.ultar.pdf";
/// Longest wait for a mesh call (best effort; failures are ignored).
const TIMEOUT: Duration = Duration::from_millis(300);

/// Tells the mesh what is open; `uri` ends with `#page=N` so the page travels with it.
/// Said again only when the URI changed. Without the mesh nothing happens.
pub fn report(bus: &Client, uri: &str, title: &str, reported: &mut Option<String>) {
    if reported.as_ref() == Some(&uri.to_owned()) {
        return;
    }
    let activity = Activity {
        app: APP_ID.to_owned(),
        title: title.to_owned(),
        uri: Some(uri.to_owned()),
        file: None,
    };
    let set = SetActivity {
        activity: Some(activity),
    };
    let _ = bus.call(&set, TIMEOUT);
    *reported = Some(uri.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::{Call, Message};

    #[test]
    fn the_contract_is_the_mesh_service() {
        // SetActivity is a versioned call to `mesh.set_activity`.
        assert_eq!(
            (
                <SetActivity as Call>::SERVICE,
                <SetActivity as Call>::METHOD
            ),
            ("mesh", "set_activity")
        );
        assert_eq!(<SetActivity as Message>::VERSION, 1);
    }
}
