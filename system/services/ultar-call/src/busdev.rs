//! The call service's [`Device`] on the bus: the account (`account.state`) says whether and where
//! the person is signed in; the account service proves and signs with the device key and hands
//! out the relay's credentials (`account.device_*`) – the key itself never comes here. Sound goes
//! through the sound card's running streams ([`crate::pcm`]), pictures through the camera node
//! ([`crate::camera`]); the other side's picture goes to the phone app (`call.picture`).

use crate::camera::{CallVideo, Camera, PictureSink};
use crate::e2e::engine::Signer;
use crate::e2e::media::{AudioIo, VideoIo};
use crate::e2e::net::TurnConfig;
use crate::e2e::pins::Pins;
use crate::{Device, SignalAuth};
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::account_device::{
    DeviceAnswer, DeviceKey, SignCall, SignalAnswer, SignalHello, Turn, CALL_SIGN_LABEL,
};
use ultar_bus::messages::account_service::{AccountState, Phase};
use ultar_bus::messages::call::{CallPicture, MAX_PICTURE};
use ultar_bus::Client;

/// How long the account service may take (a TURN credential goes to the server).
const WAIT: Duration = Duration::from_secs(10);

/// The device as the call service sees it.
pub struct BusDevice {
    client: Client,
    account: Arc<Mutex<Option<AccountState>>>,
    dir: PathBuf,
    pcm: PathBuf,
    camera: PathBuf,
    /// Where the call's voice goes while it is answered on another device.
    bridge: crate::mesh_tap::Bridge,
}

impl BusDevice {
    /// Follows `account.state` on its own thread; `dir` is the service's folder (pinned keys),
    /// `pcm` the sound node, `camera` the camera node.
    pub fn start(client: Client, dir: PathBuf, pcm: PathBuf, camera: PathBuf) -> Arc<Self> {
        Self::start_with(client, dir, pcm, camera, crate::mesh_tap::Bridge::default())
    }

    /// [`BusDevice::start`] with the `bridge` the mesh relay switches the call's voice with.
    pub fn start_with(
        client: Client,
        dir: PathBuf,
        pcm: PathBuf,
        camera: PathBuf,
        bridge: crate::mesh_tap::Bridge,
    ) -> Arc<Self> {
        let account = Arc::new(Mutex::new(None));
        if let Ok(sub) = client.subscribe::<AccountState>() {
            let a = account.clone();
            let _ = std::thread::Builder::new()
                .name("account-state".into())
                .spawn(move || {
                    while let Ok(st) = sub.recv() {
                        // a new state replaces the old one
                        if let Ok(mut g) = a.lock() {
                            *g = Some(st);
                        }
                    }
                });
        }
        Arc::new(Self {
            client,
            account,
            dir,
            pcm,
            camera,
            bridge,
        })
    }

    fn ask<C: ultar_bus::Call<Reply = ultar_bus::messages::account_device::DeviceReply>>(
        client: &Client,
        call: &C,
    ) -> Option<DeviceAnswer> {
        client.call(call, WAIT).ok()?.outcome.ok()
    }
}

/// `https://host` of a signed-in account.
fn server_of(st: &AccountState) -> Option<String> {
    matches!(st.phase, Phase::SignedIn | Phase::Offline)
        .then(|| format!("https://{}", st.server))
        .filter(|_| !st.server.is_empty())
}

/// The pictures of a call for the phone app (`call.picture`).
struct BusPictures(Client);

impl PictureSink for BusPictures {
    fn show(&mut self, own: bool, jpeg: Vec<u8>) {
        if jpeg.len() <= MAX_PICTURE {
            let _ = self.0.publish_unconfirmed(&CallPicture { own, jpeg });
        }
    }
}

struct BusAuth(Client);

impl SignalAuth for BusAuth {
    fn hello(&self) -> Result<String, String> {
        match BusDevice::ask(&self.0, &SignalHello {}) {
            Some(DeviceAnswer::Frame(f)) => Ok(f),
            _ => Err("device_key".into()),
        }
    }
    fn answer(&self, challenge: &str) -> Result<String, String> {
        let call = SignalAnswer {
            challenge: challenge.to_owned(),
        };
        match BusDevice::ask(&self.0, &call) {
            Some(DeviceAnswer::Frame(f)) => Ok(f),
            _ => Err("auth".into()),
        }
    }
}

struct BusSigner(Client);

impl Signer for BusSigner {
    fn public(&self) -> Option<[u8; 32]> {
        match BusDevice::ask(&self.0, &DeviceKey {})? {
            DeviceAnswer::Key(k) => Some(k),
            _ => None,
        }
    }
    fn sign(&self, message: &[u8]) -> Option<[u8; 64]> {
        // only the call label and a 32-byte hash can be signed; the account service adds the label
        let hash = message.strip_prefix(CALL_SIGN_LABEL)?;
        let call = SignCall {
            transcript: hash.try_into().ok()?,
        };
        match BusDevice::ask(&self.0, &call)? {
            DeviceAnswer::Signature(s) => s.try_into().ok(),
            _ => None,
        }
    }
}

impl Device for BusDevice {
    fn server(&self) -> Option<String> {
        self.account.lock().ok()?.as_ref().and_then(server_of)
    }
    fn auth(&self) -> Option<Box<dyn SignalAuth>> {
        Some(Box::new(BusAuth(self.client.clone())))
    }
    fn signer(&self) -> Box<dyn Signer> {
        Box::new(BusSigner(self.client.clone()))
    }
    fn turn(&self) -> Option<TurnConfig> {
        let DeviceAnswer::Turn {
            server,
            user,
            password,
        } = Self::ask(&self.client, &Turn {})?
        else {
            return None;
        };
        let server = server.to_socket_addrs().ok()?.find(|a| a.is_ipv4())?;
        Some(TurnConfig {
            server,
            user,
            password,
        })
    }
    fn audio(&self) -> Box<dyn AudioIo> {
        let card: Box<dyn AudioIo> = match crate::pcm::PcmAudio::open(&self.pcm) {
            Ok(a) => Box::new(a),
            Err(_) => Box::new(crate::pcm::Silent),
        };
        self.bridge.audio(card)
    }
    fn video(&self) -> Option<Box<dyn VideoIo>> {
        // without a camera the call still shows the other side
        Some(Box::new(CallVideo {
            camera: Camera::open(&self.camera).ok(),
            sink: Box::new(BusPictures(self.client.clone())),
        }))
    }
    fn pins(&self) -> Pins {
        Pins::open(self.dir.join("pins"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_signed_in_account_gives_a_server() {
        let mut st = AccountState {
            server: "ultar.code-craft.tech".into(),
            ..AccountState::default()
        };
        assert_eq!(server_of(&st), None);
        st.phase = Phase::SignedIn;
        assert_eq!(
            server_of(&st).as_deref(),
            Some("https://ultar.code-craft.tech")
        );
        st.phase = Phase::Offline;
        assert!(server_of(&st).is_some());
        st.server.clear();
        assert_eq!(server_of(&st), None);
    }
}
