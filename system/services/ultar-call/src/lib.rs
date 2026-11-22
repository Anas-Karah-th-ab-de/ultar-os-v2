//! Anrufe/Video zwischen Ultar-Geräten.
//!
//! Signalisierung läuft über den Cloud-Server (`/v1/signal`, WebSocket): Austausch von SDP-Offer/
//! Answer und ICE-Kandidaten zwischen zwei Ultar-Nummern. Die Medien gehen Ende-zu-Ende per
//! WebRTC (DTLS-SRTP) — der Server sieht sie nie; NAT-Durchstoß per STUN/TURN.
//!
//! Standard-Backend ist ein Mock (Zustandsmaschine, kein Netz). Der echte WebRTC-Pfad steckt
//! hinter dem Feature `real` (webrtc + tokio-tungstenite) und wird im Gerätebau aktiviert.

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
    Ringing,
    Answered,
    Ended,
    Error(String),
}

/// Einheitliche Schnittstelle für die Telefon-App.
pub trait CallBackend: Send {
    /// Beim Signalserver anmelden (eigene Nummer + Konto-Token + Serverbasis).
    fn register(&mut self, number: &str, token: &str, server_base: &str);
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
}

/// Mock ohne Netz: lokale Zustandsmaschine, damit die Oberfläche vollständig läuft.
pub struct MockCall {
    state: CallState,
    events: Vec<CallEvent>,
    registered: bool,
}

impl Default for MockCall {
    fn default() -> Self {
        Self { state: CallState::Idle, events: Vec::new(), registered: false }
    }
}

impl CallBackend for MockCall {
    fn register(&mut self, _number: &str, _token: &str, _server_base: &str) {
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

#[cfg(feature = "real")]
mod real;
#[cfg(feature = "real")]
pub use real::WebrtcCall;

/// Standard-Backend je nach Build.
pub fn default_backend() -> Box<dyn CallBackend> {
    #[cfg(feature = "real")]
    {
        Box::new(real::WebrtcCall::default())
    }
    #[cfg(not(feature = "real"))]
    {
        Box::new(MockCall::default())
    }
}
