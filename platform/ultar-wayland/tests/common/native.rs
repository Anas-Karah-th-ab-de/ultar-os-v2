//! A raw native client of the compositor (another app on screen) for the
//! clipboard tests.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use ultar_compositor::Compositor;
use ultar_surface::proto::{
    BufferId, Event, PixelFormat, Rect, Request, Role, SurfaceId, PROTOCOL_VERSION,
};
use ultar_surface::wire::FrameReader;

pub struct Peer {
    stream: UnixStream,
    reader: FrameReader,
    out: Vec<u8>,
    pub events: Vec<Event>,
}

impl Peer {
    pub fn connect(comp: &mut Compositor) -> Peer {
        let (a, b) = UnixStream::pair().unwrap();
        comp.add_stream(a).expect("accepted");
        b.set_nonblocking(true).unwrap();
        let mut p = Peer {
            stream: b,
            reader: FrameReader::new(),
            out: Vec::new(),
            events: Vec::new(),
        };
        p.send(&Request::Hello {
            version: PROTOCOL_VERSION,
            app_id: "peer".into(),
            shared_memory: false,
        });
        p
    }

    pub fn send(&mut self, r: &Request) {
        self.out.extend_from_slice(&r.encode().unwrap());
    }

    /// Writes, lets the compositor work, reads.
    pub fn pump(&mut self, comp: &mut Compositor) {
        for _ in 0..20 {
            while !self.out.is_empty() {
                match self.stream.write(&self.out) {
                    Ok(n) if n > 0 => {
                        self.out.drain(..n);
                    }
                    _ => break,
                }
            }
            comp.dispatch(Some(std::time::Duration::from_millis(1)))
                .unwrap();
            let mut buf = [0u8; 65536];
            while let Ok(n) = self.stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                self.reader.push(&buf[..n]);
            }
            while let Ok(Some((op, body))) = self.reader.next_frame() {
                if let Ok(e) = Event::decode(op, &body) {
                    if let Event::Ping { serial } = e {
                        self.send(&Request::Pong { serial });
                    }
                    self.events.push(e);
                }
            }
        }
    }

    /// A window of `color`, `size` pixels (inline pixels).
    pub fn window(&mut self, comp: &mut Compositor, size: (u32, u32), color: u32) -> SurfaceId {
        let s = SurfaceId(1);
        self.send(&Request::CreateSurface {
            surface: s,
            role: Role::Toplevel,
        });
        self.pump(comp);
        let serial = self
            .events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Configure(c) if c.surface == s => Some(c.serial),
                _ => None,
            })
            .unwrap_or(0);
        let (w, h) = size;
        self.send(&Request::CreateInlineBuffer {
            buffer: BufferId(1),
            width: w,
            height: h,
            format: PixelFormat::Xrgb8888,
        });
        let data: Vec<u8> = std::iter::repeat_n(color.to_le_bytes(), (w * h) as usize)
            .flatten()
            .collect();
        self.send(&Request::WritePixels {
            buffer: BufferId(1),
            rect: Rect::new(0, 0, w, h),
            data,
        });
        self.send(&Request::Attach {
            surface: s,
            buffer: BufferId(1),
        });
        self.send(&Request::Damage {
            surface: s,
            rects: vec![Rect::new(0, 0, w, h)],
        });
        self.send(&Request::Commit {
            surface: s,
            configure: serial,
        });
        self.pump(comp);
        s
    }

    /// The newest serial of a button or key press (releases are no user action).
    pub fn last_serial(&self) -> u32 {
        self.events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::PointerButton {
                    serial,
                    pressed: true,
                    ..
                }
                | Event::Key {
                    serial, state: 1, ..
                } => Some(*serial),
                _ => None,
            })
            .unwrap_or(0)
    }
}
