//! Calls between Ultar devices, end to end: the signalling server and the TURN relay carry
//! only sealed bytes and know nothing about what is said, shown or where it goes beyond the two
//! numbers and the relay addresses. Nothing about a call is kept on the server.
//!
//! * [`crypto`] – keys of a call from the two device keys (X25519, Ed25519, ChaCha20-Poly1305);
//! * [`engine`] – handshake and sealed signalling as a state machine;
//! * [`pins`] – the device key seen for each number (safety number, changed-key warning);
//! * [`stun`], [`net`] – the UDP socket: own addresses, TURN relay;
//! * [`media`] – way finding and the running call; [`audio`], [`video`] – voice and picture.

pub mod audio;
pub mod backend;
pub mod crypto;
pub mod engine;
pub mod media;
pub mod net;
pub mod pins;
pub mod replay;
pub mod signal;
pub mod stun;
pub mod video;
