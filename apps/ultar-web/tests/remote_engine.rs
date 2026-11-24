//! The engine-host side of the protocol with a stand-in host on the other end of a socket:
//! readiness, commands, a shared-memory frame passed with SCM_RIGHTS and read back by rows,
//! refusal of a host that breaks the protocol, and a missing host program.

use rustix::net::{
    sendmsg, socketpair, AddressFamily, SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
    SocketFlags, SocketType,
};
use std::io::{IoSlice, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;
use ultar_web::engine::remote::{HostConfig, RemoteEngine};
use ultar_web::engine::{EngineError, EngineEvent, FrameDamage, TabId, WebEngine};
use ultar_web::ipc::{Command, Decoder, HostEvent, PROTOCOL_VERSION};

fn pair() -> (OwnedFd, UnixStream) {
    let (a, b) = socketpair(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    (a, UnixStream::from(b))
}

fn read_command(host: &mut UnixStream) -> Command {
    let mut len = [0u8; 4];
    host.read_exact(&mut len).unwrap();
    let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
    host.read_exact(&mut body).unwrap();
    let mut d = Decoder::default();
    d.push(&len);
    d.push(&body);
    let (kind, payload) = d.next_message().unwrap().unwrap();
    // Commands are not decoded by the browser; compare by encoding.
    for c in [
        Command::CreateTab(TabId(1), false),
        Command::FrameDone(TabId(1)),
        Command::Quit,
    ] {
        let e = c.encode();
        if e[4] == kind && e[5..] == payload[..] {
            return c;
        }
    }
    panic!("unexpected command kind {kind}");
}

fn send_with_fd(host: &UnixStream, bytes: &[u8], fd: &OwnedFd) {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let fds = [fd.as_fd()];
    assert!(control.push(SendAncillaryMessage::ScmRights(&fds)));
    sendmsg(
        host,
        &[IoSlice::new(bytes)],
        &mut control,
        SendFlags::empty(),
    )
    .unwrap();
}

#[test]
fn a_frame_travels_through_shared_memory() {
    let (ours, mut host) = pair();
    host.write_all(&HostEvent::Ready(PROTOCOL_VERSION).encode())
        .unwrap();
    let mut engine = RemoteEngine::with_socket(ours);
    engine.wait_ready(Duration::from_secs(5)).unwrap();
    let tab = engine.create_tab(false).unwrap();
    assert_eq!(read_command(&mut host), Command::CreateTab(TabId(1), false));
    // A 4 × 2 frame with a padded stride of 20 bytes.
    let memfd = rustix::fs::memfd_create("frame", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    let mut file = std::fs::File::from(memfd.try_clone().unwrap());
    let pixels: Vec<u8> = (0..40u8).collect();
    file.write_all(&pixels).unwrap();
    send_with_fd(&host, &HostEvent::Buffer(tab, 4, 2, 20).encode(), &memfd);
    let damage = FrameDamage {
        x0: 1,
        y0: 1,
        x1: 3,
        y1: 2,
    };
    host.write_all(&HostEvent::Engine(EngineEvent::Frame { tab, damage }).encode())
        .unwrap();
    let mut events = Vec::new();
    for _ in 0..20 {
        events.extend(engine.poll(Some(Duration::from_millis(200))).unwrap());
        if !events.is_empty() {
            break;
        }
    }
    assert_eq!(events, vec![EngineEvent::Frame { tab, damage }]);
    assert_eq!(read_command(&mut host), Command::FrameDone(tab));
    let frame = engine.frame(tab).unwrap();
    assert_eq!((frame.width, frame.height, frame.stride), (4, 2, 20));
    // Only the damaged pixels (row 1, columns 1..3) were read.
    assert_eq!(&frame.pixels[24..32], &pixels[24..32]);
    assert_eq!(&frame.pixels[0..24], &[0u8; 24]);
    assert!(matches!(
        engine.load(TabId(9), "https://a/"),
        Err(EngineError::UnknownTab(_))
    ));
    drop(engine);
    assert_eq!(read_command(&mut host), Command::Quit);
}

#[test]
fn a_host_that_breaks_the_protocol_is_dropped() {
    let (ours, mut host) = pair();
    host.write_all(&HostEvent::Ready(PROTOCOL_VERSION + 1).encode())
        .unwrap();
    let mut engine = RemoteEngine::with_socket(ours);
    assert!(engine.wait_ready(Duration::from_secs(5)).is_err());
    let (ours, mut host2) = pair();
    let mut engine = RemoteEngine::with_socket(ours);
    host2.write_all(&[0, 0, 0, 0, 1]).unwrap();
    assert!(matches!(
        engine.poll(Some(Duration::from_secs(2))),
        Err(EngineError::Protocol(_))
    ));
    // A buffer message without a descriptor.
    let (ours, mut host3) = pair();
    let mut engine = RemoteEngine::with_socket(ours);
    engine.create_tab(false).unwrap();
    host3
        .write_all(&HostEvent::Buffer(TabId(1), 2, 2, 8).encode())
        .unwrap();
    assert!(engine.poll(Some(Duration::from_secs(2))).is_err());
    drop(host);
}

#[test]
fn a_missing_host_program_is_a_start_error() {
    let config = HostConfig {
        program: "/nonexistent/ultar-web-engine".into(),
        cell: None,
        profile: "/tmp".into(),
        downloads: "/tmp".into(),
        user_agent: "UltarWeb/0.7.0".into(),
        filter: None,
    };
    assert!(config.args().iter().any(|a| a == "--no-cell"));
    assert!(matches!(
        RemoteEngine::start(&config),
        Err(EngineError::Start(_))
    ));
}
