//! Telling the shell that a file arrived (`mesh.received`): the mesh opens it once by itself
//! with the default app; the notice lets the person open it again ("Open" on the toast).

use crate::node::Node;
use ultar_bus::messages::mesh_received::FileReceived;

/// Longest name of a file or device passed on.
const MAX_NAME: usize = 128;

/// The notice for what [`crate::node::Effect::Open`] is about to open, when that is a file
/// that came from another device (`app` empty: the default app opens it). `None` for an
/// activity that continues in an app.
pub fn file_received(node: &Node, offer: u64, app: &str, uri: Option<&str>) -> Option<FileReceived> {
    let uri = uri.filter(|_| app.is_empty())?;
    let from_name = node
        .offers
        .received
        .get(&offer)
        .and_then(|got| node.links.get(&got.peer))
        .and_then(|link| link.device.as_ref())
        .map(|device| device.name.clone())
        .unwrap_or_default();
    let title = uri.rsplit('/').next().unwrap_or(uri);
    Some(FileReceived {
        offer,
        from_name: from_name.chars().take(MAX_NAME).collect(),
        title: title.chars().take(MAX_NAME).collect(),
        uri: uri.to_owned(),
    })
}
