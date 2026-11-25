//! What pages may learn and reach. Two rules for every engine of the browser:
//!
//! * **Nothing true about the device or the person leaves it** ([`persona`]): every page sees
//!   the same ordinary identity (the one Tor Browser shows: Firefox on Windows, English, UTC,
//!   a common screen size); asked for the position, it gets an invented one; the browser's own
//!   language, the system, the hardware, the sound card, the network address stay unknown.
//!   The address the site sees is a Tor exit ([`crate::engine::reader::tor`]).
//! * **Nothing from the internet reaches the system** ([`guard`]): only `http`/`https`; no
//!   address of this device or the local network (also not after a redirect); pages run no
//!   program; the browser itself runs in its own cell without rights to the system.

pub mod guard;
pub mod persona;
