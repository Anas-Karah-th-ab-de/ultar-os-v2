//! The bus side of "play there": `mesh.media_open`, `mesh.media_pcm` and `mesh.media_ctl`
//! of the local media service, answered through the node.

use crate::service::{serve, MeshService};
use ultar_bus::messages::mesh::Done;
use ultar_bus::messages::mesh_media::{MediaCtlOut, MediaOpen, MediaOpened, MediaPcmOut};
use ultar_bus::{Call, IncomingRequest};
use ultar_svckit::Bus;

impl MeshService {
    /// The media calls; gives the request back when it is another method.
    pub(crate) fn media_calls(
        &mut self,
        request: IncomingRequest,
        bus: &Bus<'_, Self>,
    ) -> Option<IncomingRequest> {
        let method = request.method().to_owned();
        if method == MediaOpen::METHOD {
            serve::<MediaOpen>(request, |c, _| {
                let (id, effects) = self.runtime.node.media_open(&c.device, &c.title, &c.artist);
                self.act(effects, bus);
                MediaOpened { id }
            });
        } else if method == MediaPcmOut::METHOD {
            serve::<MediaPcmOut>(request, |c, _| {
                let effects = match crate::device::decode(&c.pcm) {
                    Some(pcm) => self.runtime.node.media_pcm(&c.device, c.id, c.seq, &pcm),
                    None => Vec::new(),
                };
                let ok = !effects.is_empty();
                self.act(effects, bus);
                Done { ok }
            });
        } else if method == MediaCtlOut::METHOD {
            serve::<MediaCtlOut>(request, |c, _| {
                let effects = self.runtime.node.media_ctl(&c.device, c.id, c.ctl);
                let ok = !effects.is_empty();
                self.act(effects, bus);
                Done { ok }
            });
        } else {
            return Some(request);
        }
        None
    }
}
