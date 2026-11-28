//! # ultar-web-engine – the Servo engine host of the Ultar Browser
//!
//! **Area:** apps (area 7). Started only by the browser (`apps/ultar-web`,
//! `engine::remote::RemoteEngine`) with the browser socket as standard input. Renders the pages
//! of the browser's tabs with Servo (HTML, CSS, JavaScript) in software, hands the pixels over
//! as shared memory and speaks the protocol of `apps/ultar-web/src/ipc`.
//!
//! * **The internet only through Tor:** Servo sends every connection to the browser's HTTP
//!   `CONNECT` port (`--proxy`, with the browser's secret); that port refuses this device and the
//!   local network and goes out only through Tor. A host started without a proxy refuses to run.
//! * **The persona** (`ultar_web::privacy::persona`): user agent, language, time zone, screen,
//!   clocks and canvas read-back are the same for everyone; WebRTC, Bluetooth, gamepads, WebGPU,
//!   WebXR and service workers are off; the location is the invented place of the session.
//! * **Nothing is kept:** cookies, storage and cache live in memory for the session.
//! * **Fail closed:** the page cell (`--cell-*`) is not built by this host yet, so it refuses to
//!   start with one; the browser then shows pages with its reader engine.
//!
//! Exit codes: 0 the browser said `Quit` or went away, 1 failed, 2 usage.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod args;
mod delegate;
mod frames;
mod host;
mod input;
mod link;
mod persona;
mod waker;

use std::process::ExitCode;
use ultar_web::privacy::persona::Persona;

// the runtime bundle (environment of the libraries) and the sound card
mod media;
#[cfg(any(feature = "media", test))]
mod pcm;
mod runtime;

fn main() -> ExitCode {
    // before any thread: the bundle's fonts and Mesa drivers; then the sound sink (feature media)
    runtime::setup();
    media::install();
    let Some(args) = args::parse(std::env::args_os().skip(1)) else {
        eprintln!("ultar-web-engine: started with unknown options (only the browser starts it)");
        return ExitCode::from(2);
    };
    let Some(proxy) = args.proxy.as_deref() else {
        eprintln!("ultar-web-engine: no proxy: nothing may go out without Tor");
        return ExitCode::from(1);
    };
    if !args.cell.none {
        eprintln!("ultar-web-engine: the page cell is not supported by this host yet");
        return ExitCode::from(1);
    }
    let link = match link::Link::from_stdin() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("ultar-web-engine: no browser socket: {e}");
            return ExitCode::from(1);
        }
    };
    let persona = Persona::for_session(Persona::new_seed());
    let result = host::Host::new(link, persona, proxy).and_then(|mut h| h.run());
    match result {
        Ok(()) | Err(host::HostError::Link(link::LinkError::Closed)) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ultar-web-engine: {e}");
            ExitCode::from(1)
        }
    }
}
