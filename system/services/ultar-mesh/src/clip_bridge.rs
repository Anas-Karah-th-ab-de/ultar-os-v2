//! The mesh as the clipboard bridge of the compositor (right `ui.clipboard-bridge`,
//! granted to the mesh service's user only, `compositor-rights.toml`).
//!
//! One thread holds the connection: it is told when the person copied something
//! (`Event::SelectionChange`), reads the text or picture of that selection and hands it to
//! the mesh ([`Copied`]); and it offers what another device copied as the local selection
//! and gives its data when an app pastes. The compositor never tells it of a confidential
//! copy (a password) and gives it nothing while the session is locked; this side drops
//! such a change too. Without the compositor (or without the right) it keeps trying
//! quietly and the mesh simply shares nothing.

use crate::clip_image;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};
use ultar_surface::client::Client;
use ultar_surface::proto::{
    Event, Request, Rights, TransferId, CLIP_IMAGE_MIME, CLIP_TEXT_MIME, MAX_CHUNK,
};

/// Longest text of the clipboard that travels.
pub const TEXT_MAX: usize = 16 * 1024;
/// Time between two tries to reach the compositor.
const RETRY: Duration = Duration::from_secs(2);
/// How long the bridge waits for the data of a copy.
const READ_TIME: Duration = Duration::from_secs(5);
/// How often the connection is looked at while nothing happens.
const POLL: Duration = Duration::from_millis(100);
/// Most transfers the bridge serves at once.
const MAX_SERVING: usize = 8;

/// What the person copied on this device.
#[derive(Debug, PartialEq, Eq)]
pub enum Copied {
    /// Text.
    Text(String),
    /// A picture (PNG).
    Image(Vec<u8>),
}

enum Command {
    Text(String),
    Image(Vec<u8>),
    Clear,
    Stop,
}

/// The connection to the compositor (a thread; dropping it ends the thread).
pub struct Bridge {
    commands: mpsc::Sender<Command>,
}

impl Bridge {
    /// Connects (and reconnects) to the compositor at `socket`. `deliver` gets what the
    /// person copies and returns `false` when nobody listens any more.
    pub fn start(
        socket: PathBuf,
        deliver: impl Fn(Copied) -> bool + Send + 'static,
    ) -> Option<Bridge> {
        let (commands, inbox) = mpsc::channel();
        std::thread::Builder::new()
            .name("mesh-clip-bridge".into())
            .spawn(move || run(&socket, &inbox, &deliver))
            .ok()?;
        Some(Bridge { commands })
    }

    /// Offers text that was copied on another device.
    pub fn offer_text(&self, text: String) {
        let _ = self.commands.send(Command::Text(text));
    }

    /// Offers a picture that was copied on another device.
    pub fn offer_image(&self, png: Vec<u8>) {
        let _ = self.commands.send(Command::Image(png));
    }

    /// Takes back the offer of another device's entry (if it is still the clipboard).
    pub fn clear(&self) {
        let _ = self.commands.send(Command::Clear);
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

enum End {
    Stop,
    Lost,
}

fn run(
    socket: &std::path::Path,
    inbox: &mpsc::Receiver<Command>,
    deliver: &dyn Fn(Copied) -> bool,
) {
    loop {
        let Some(mut client) = connect(socket, inbox) else {
            return;
        };
        if let End::Stop = serve(&mut client, inbox, deliver) {
            return;
        }
    }
}

/// A connection that has the bridge right; waits and tries again until there is one
/// (`None`: the mesh ended). Offers that come while there is none are dropped.
fn connect(socket: &std::path::Path, inbox: &mpsc::Receiver<Command>) -> Option<Client> {
    loop {
        if let Ok(client) = Client::connect(socket, "ultar-mesh") {
            if client.rights().contains(Rights::CLIPBOARD_BRIDGE) {
                return Some(client);
            }
        }
        match inbox.recv_timeout(RETRY) {
            Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return None,
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn serve(
    client: &mut Client,
    inbox: &mpsc::Receiver<Command>,
    deliver: &dyn Fn(Copied) -> bool,
) -> End {
    let mut session = Session::default();
    loop {
        loop {
            match inbox.try_recv() {
                Ok(Command::Stop) | Err(TryRecvError::Disconnected) => return End::Stop,
                Ok(command) => {
                    if session.command(client, command).is_err() {
                        return End::Lost;
                    }
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        let step = match client.next_event(Some(POLL)) {
            Ok(Some(event)) => session.event(client, event, deliver),
            Ok(None) => session.tick(client),
            Err(_) => return End::Lost,
        };
        match step {
            Ok(true) => {}
            Ok(false) => return End::Stop,
            Err(()) => return End::Lost,
        }
    }
}

/// What this side offers for another device.
struct Offer {
    mime: &'static str,
    data: Vec<u8>,
}

/// A copy that is being read.
struct Reading {
    mime: &'static str,
    bytes: Vec<u8>,
    since: Instant,
}

#[derive(Default)]
struct Session {
    offer: Option<Offer>,
    reading: Option<Reading>,
    /// The newest change that came while a read was going.
    waiting: Option<Vec<String>>,
    /// Offsets of the transfers being served.
    serving: BTreeMap<u32, usize>,
}

impl Session {
    fn command(&mut self, client: &mut Client, command: Command) -> Result<(), ()> {
        let (mime, data) = match command {
            Command::Text(text) if !text.is_empty() && text.len() <= TEXT_MAX => {
                (CLIP_TEXT_MIME, text.into_bytes())
            }
            Command::Image(png)
                if clip_image::is_png(&png) && png.len() <= clip_image::MAX_IMAGE =>
            {
                (CLIP_IMAGE_MIME, png)
            }
            Command::Clear => {
                // Only what this side offered; a newer copy of the person stays.
                if self.offer.take().is_some() {
                    client.bridge_set_selection(&[]).map_err(|_| ())?;
                }
                return Ok(());
            }
            _ => return Ok(()),
        };
        client
            .bridge_set_selection(&[mime.to_owned()])
            .map_err(|_| ())?;
        self.offer = Some(Offer {
            mime: mime_static(mime),
            data,
        });
        Ok(())
    }

    /// `Ok(false)`: the mesh does not listen any more.
    fn event(
        &mut self,
        client: &mut Client,
        event: Event,
        deliver: &dyn Fn(Copied) -> bool,
    ) -> Result<bool, ()> {
        match event {
            Event::SelectionChange {
                mimes,
                confidential,
            } => {
                // The person copied something: an offer of another device is gone.
                self.offer = None;
                self.waiting = None;
                if !confidential {
                    self.start_read(client, mimes)?;
                }
            }
            Event::Data { data, last, .. } => {
                let Some(reading) = self.reading.as_mut() else {
                    return Ok(true);
                };
                let limit = if reading.mime == CLIP_IMAGE_MIME {
                    clip_image::MAX_IMAGE
                } else {
                    TEXT_MAX
                };
                if reading.bytes.len() + data.len() > limit {
                    self.reading = None;
                } else {
                    reading.bytes.extend_from_slice(&data);
                    if last {
                        let done = self.reading.take();
                        if let Some(done) = done {
                            if !hand_over(done, deliver) {
                                return Ok(false);
                            }
                        }
                    }
                }
                if self.reading.is_none() {
                    self.next(client)?;
                }
            }
            Event::TransferCancelled { .. } | Event::Error { .. } => {
                self.reading = None;
                self.next(client)?;
            }
            Event::SendRequest { transfer, mime } => self.serve_chunk(client, transfer, &mime)?,
            _ => {}
        }
        Ok(true)
    }

    fn tick(&mut self, client: &mut Client) -> Result<bool, ()> {
        let late = self
            .reading
            .as_ref()
            .is_some_and(|r| r.since.elapsed() > READ_TIME);
        if late {
            self.reading = None;
            self.next(client)?;
        }
        Ok(true)
    }

    /// Reads the copy (text before picture); a second change waits for the first.
    fn start_read(&mut self, client: &mut Client, mimes: Vec<String>) -> Result<(), ()> {
        if self.reading.is_some() {
            self.waiting = Some(mimes);
            return Ok(());
        }
        let mime = if mimes.iter().any(|m| m == CLIP_TEXT_MIME) {
            CLIP_TEXT_MIME
        } else if mimes.iter().any(|m| m == CLIP_IMAGE_MIME) {
            CLIP_IMAGE_MIME
        } else {
            return Ok(());
        };
        client.bridge_read_selection(mime).map_err(|_| ())?;
        self.reading = Some(Reading {
            mime: mime_static(mime),
            bytes: Vec::new(),
            since: Instant::now(),
        });
        Ok(())
    }

    fn next(&mut self, client: &mut Client) -> Result<(), ()> {
        match self.waiting.take() {
            Some(mimes) => self.start_read(client, mimes),
            None => Ok(()),
        }
    }

    /// Gives the next chunk of the offered entry (one per request: the compositor asks again
    /// when the reader has room).
    fn serve_chunk(
        &mut self,
        client: &mut Client,
        transfer: TransferId,
        mime: &str,
    ) -> Result<(), ()> {
        let data: &[u8] = match &self.offer {
            Some(o) if o.mime == mime => &o.data,
            _ => &[],
        };
        if self.serving.len() >= MAX_SERVING && !self.serving.contains_key(&transfer.0) {
            self.serving.clear();
        }
        let start = (*self.serving.entry(transfer.0).or_insert(0)).min(data.len());
        let end = (start + MAX_CHUNK).min(data.len());
        let last = end == data.len();
        if last {
            self.serving.remove(&transfer.0);
        } else {
            self.serving.insert(transfer.0, end);
        }
        client
            .send(&Request::SendData {
                transfer,
                data: data[start..end].to_vec(),
                last,
            })
            .map_err(|_| ())
    }
}

fn mime_static(mime: &str) -> &'static str {
    if mime == CLIP_IMAGE_MIME {
        CLIP_IMAGE_MIME
    } else {
        CLIP_TEXT_MIME
    }
}

/// Hands a finished read to the mesh; `false` when nobody listens.
fn hand_over(done: Reading, deliver: &dyn Fn(Copied) -> bool) -> bool {
    let copied = if done.mime == CLIP_IMAGE_MIME {
        clip_image::is_png(&done.bytes).then_some(Copied::Image(done.bytes))
    } else {
        String::from_utf8(done.bytes)
            .ok()
            .filter(|t| !t.is_empty())
            .map(Copied::Text)
    };
    copied.is_none_or(deliver)
}
