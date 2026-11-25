//! Call service of Ultar OS (area: system services).
//!
//! Two parts:
//! * The **call service** on the bus: [`service::Service`] answers the contract
//!   `ultar_bus::messages::call` (`call.dial`, `call.answer`, … and the retained topic
//!   `call.state`) through a [`session::Session`] over one [`Line`] per way (Ultar number,
//!   mobile network) and an [`line::AudioRoute`]. [`fake`] is the test double of every backend.
//! * The device-to-device backend [`CallBackend`]: [`E2eCall`] ([`e2e`]) – signalling over the
//!   account server, keys from the two device keys, voice and picture sealed end to end, directly
//!   or through the TURN relay; [`MockCall`] for user interface work.
//!   [`ultar_line::UltarLine`] makes it the Ultar-number [`Line`].
//!
//! The server relays and forgets: it never sees voice, picture or addresses in the clear and
//! keeps nothing about a call.

pub mod busdev;
pub mod calllog;
pub mod camera;
pub mod e2e;
pub mod emergency;
pub mod fake;
pub mod fake_modem;
pub mod line;
pub mod logstore;
pub mod modem_line;
pub mod msg;
pub mod pcm;
pub mod service;
pub mod session;
mod session_cmd;
pub mod ultar_line;

pub use line::Line;

#[derive(Clone, Debug, PartialEq)]
pub enum CallState {
    Idle,
    Dialing(String),
    Incoming(String),
    Active(String),
    Ended,
}

#[derive(Clone, Debug)]
pub enum CallEvent {
    Incoming(String),
    /// A call comes in; `video` when it was placed as a video call.
    IncomingCall {
        /// The caller's number (as the signalling server delivered it).
        number: String,
        /// Video call.
        video: bool,
    },
    /// The end-to-end keys are agreed (see [`e2e::crypto::safety_number`]).
    Secured {
        /// Safety number.
        safety: String,
        /// The other number's device key changed.
        changed: bool,
    },
    Ringing,
    Answered,
    Ended,
    Error(String),
    /// Security notice of the server about the account's devices (for example
    /// `recovery_started`: somebody asked to move a number to another device). `device` holds the
    /// first characters of the device id concerned. The user interface must show it.
    Notice {
        event: String,
        device: String,
    },
}

/// Proof of the device key towards the signalling server (docs/DEVICE-IDENTITY.md).
/// The key belongs to the cloud service; whoever wires the two services together implements this.
pub trait SignalAuth: Send {
    /// First frame: names the device.
    fn hello(&self) -> Result<String, String>;
    /// Signed answer to the server's challenge frame.
    fn answer(&self, challenge_frame: &str) -> Result<String, String>;
}

/// Einheitliche Schnittstelle für die Telefon-App.
pub trait CallBackend: Send {
    /// Beim Signalserver anmelden: Serverbasis und Nachweis des Geräteschlüssels.
    fn register(&mut self, server_base: &str, auth: Box<dyn SignalAuth>);
    /// Anruf starten (Video optional).
    fn place(&mut self, peer: &str, video: bool);
    /// Eingehenden Anruf annehmen.
    fn answer(&mut self);
    /// Auflegen/ablehnen.
    fn hangup(&mut self);
    /// Neue Ereignisse abholen (nicht blockierend).
    fn poll(&mut self) -> Vec<CallEvent>;
    fn state(&self) -> CallState;
    /// true, wenn beim Signalserver angemeldet.
    fn ready(&self) -> bool;
    /// The own Ultar number as the signalling server named it (none before the first sign-in).
    fn number(&self) -> Option<String> {
        None
    }
}

/// Mock ohne Netz: lokale Zustandsmaschine, damit die Oberfläche vollständig läuft.
pub struct MockCall {
    state: CallState,
    events: Vec<CallEvent>,
    registered: bool,
}

impl Default for MockCall {
    fn default() -> Self {
        Self {
            state: CallState::Idle,
            events: Vec::new(),
            registered: false,
        }
    }
}

impl CallBackend for MockCall {
    fn register(&mut self, _server_base: &str, _auth: Box<dyn SignalAuth>) {
        self.registered = true;
    }
    fn place(&mut self, peer: &str, _video: bool) {
        if peer.is_empty() {
            return;
        }
        self.state = CallState::Dialing(peer.to_string());
        self.events.push(CallEvent::Ringing);
    }
    fn answer(&mut self) {
        if let CallState::Incoming(n) = self.state.clone() {
            self.state = CallState::Active(n);
            self.events.push(CallEvent::Answered);
        }
    }
    fn hangup(&mut self) {
        self.state = CallState::Ended;
        self.events.push(CallEvent::Ended);
    }
    fn poll(&mut self) -> Vec<CallEvent> {
        std::mem::take(&mut self.events)
    }
    fn state(&self) -> CallState {
        self.state.clone()
    }
    fn ready(&self) -> bool {
        self.registered
    }
}

pub use e2e::backend::{Device, E2eCall};

/// The backend without a device (no key, no media): the local mock for user interface work.
pub fn default_backend() -> Box<dyn CallBackend> {
    Box::new(MockCall::default())
}
