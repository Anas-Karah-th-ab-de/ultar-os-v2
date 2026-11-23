use super::*;
use crate::wire::HEADER;

fn mimes() -> Vec<String> {
    vec!["text/plain;charset=utf-8".into(), "image/png".into()]
}

#[rustfmt::skip] // one request per line reads like a table
fn sample_requests() -> Vec<Request> {
    let s = SurfaceId(3);
    vec![
        Request::Hello { version: 1, app_id: "org.ultar.files".into(), shared_memory: true },
        Request::CreateSurface { surface: s, role: Role::Toplevel },
        Request::CreateSurface { surface: s, role: Role::Popup { parent: SurfaceId(1), x: -4, y: 9 } },
        Request::CreateSurface { surface: s, role: Role::Panel { edge: Edge::End, size: 56 } },
        Request::CreateSurface { surface: s, role: Role::InputPanel },
        Request::CreateSurface { surface: s, role: Role::Lock },
        Request::CreateSurface { surface: s, role: Role::Background },
        Request::CreateSurface { surface: s, role: Role::Overlay },
        Request::DestroySurface { surface: s },
        Request::SetTitle { surface: s, title: "الملفات – Files".into() },
        Request::SetSizeLimits { surface: s, min: (100, 50), max: (0, 0) },
        Request::CreateShmBuffer { buffer: BufferId(1), width: 2, height: 3, stride: 8, format: PixelFormat::Argb8888 },
        Request::CreateInlineBuffer { buffer: BufferId(2), width: 2, height: 3, format: PixelFormat::Xrgb8888 },
        Request::WritePixels { buffer: BufferId(2), rect: Rect::new(0, 1, 1, 1), data: vec![1, 2, 3, 4] },
        Request::DestroyBuffer { buffer: BufferId(2) },
        Request::Attach { surface: s, buffer: BufferId(1) },
        Request::Damage { surface: s, rects: vec![Rect::new(0, 0, 2, 3), Rect::new(1, 1, 1, 1)] },
        Request::Commit { surface: s, configure: 9 },
        Request::Frame { surface: s },
        Request::SetState { surface: s, state: WindowState::Maximized },
        Request::SetState { surface: s, state: WindowState::Minimized },
        Request::Move { surface: s, serial: 5 },
        Request::Resize { surface: s, serial: 5, edges: ResizeEdges(ResizeEdges::TOP) },
        Request::Pong { serial: 77 },
        Request::SetSelection { serial: 1, mimes: mimes() },
        Request::ReadSelection { serial: 2, mime: "text/plain".into() },
        Request::SendData { transfer: TransferId(4), data: vec![0; 10], last: true },
        Request::StartDrag { serial: 3, mimes: mimes() },
        Request::ReadDrop { mime: "image/png".into() },
        Request::TextInput { surface: s, enabled: true, purpose: 2 },
        Request::ImeCommit { text: "سلام\n".into() },
        Request::Capture { output: OutputId(0) },
    ]
}

#[rustfmt::skip] // one event per line reads like a table
fn sample_events() -> Vec<Event> {
    let s = SurfaceId(8);
    vec![
        Event::Welcome { version: 1, transport: Transport::Inline, rights: Rights::SHELL, max_buffer_bytes: 1 << 28 },
        Event::Welcome { version: 1, transport: Transport::SharedMemory, rights: Rights::NONE, max_buffer_bytes: 9 },
        Event::Output(OutputInfo { id: OutputId(1), x: 1920, y: 0, width: 3840, height: 2160, scale_milli: 2000, refresh_millihertz: 60000, primary: false, name: "HDMI-A-1".into() }),
        Event::OutputRemoved { output: OutputId(1) },
        Event::Configure(Configure { surface: s, serial: 4, width: 800, height: 600, scale_milli: 1250, state: WindowState::Fullscreen, activated: true, rtl: true }),
        Event::BufferReleased { buffer: BufferId(2) },
        Event::FrameDone { surface: s, time_ms: 16 },
        Event::PointerEnter { surface: s, x: 256, y: -256 },
        Event::PointerLeave { surface: s },
        Event::PointerMotion { surface: s, x: 1, y: 2 },
        Event::PointerButton { surface: s, button: 0x110, pressed: true, serial: 3 },
        Event::PointerAxis { surface: s, dx: 0, dy: 48 * 256 },
        Event::KeyboardFocus { surface: s, focused: false },
        Event::Key { surface: s, code: 30, state: 1, modifiers: Modifiers(Modifiers::SHIFT), character: 'ش' as u32, serial: 9 },
        Event::TouchDown { surface: s, id: 1, x: 5, y: 6, serial: 10 },
        Event::TouchMove { surface: s, id: 1, x: 7, y: 8 },
        Event::TouchUp { surface: s, id: 1 },
        Event::TouchCancel { surface: s },
        Event::Text { surface: s, text: "مرحبا".into() },
        Event::CloseRequested { surface: s },
        Event::SelectionOffer { mimes: mimes() },
        Event::SendRequest { transfer: TransferId(1), mime: "text/plain".into() },
        Event::Data { transfer: TransferId(1), data: b"abc".to_vec(), last: false },
        Event::TransferCancelled { transfer: TransferId(1) },
        Event::DropOffer { surface: s, x: 1, y: 1, mimes: mimes() },
        Event::Ping { serial: 12 },
        Event::ImeState { active: true, purpose: 1 },
        Event::CaptureData { output: OutputId(0), width: 2, height: 1, row: 0, data: vec![0; 8], last: true },
        Event::Error { code: ErrorCode::Malformed },
        Event::Error { code: ErrorCode::TooSlow },
    ]
}

fn split(frame: &[u8]) -> (u16, &[u8]) {
    (u16::from_le_bytes([frame[4], frame[5]]), &frame[HEADER..])
}

#[test]
fn every_request_round_trips_and_truncation_fails() {
    for request in sample_requests() {
        let frame = request.encode().unwrap();
        let (op, body) = split(&frame);
        assert_eq!(Request::decode(op, body).unwrap(), request);
        if !body.is_empty() {
            assert!(
                Request::decode(op, &body[..body.len() - 1]).is_err(),
                "{request:?}"
            );
        }
        let mut longer = body.to_vec();
        longer.push(0);
        assert!(Request::decode(op, &longer).is_err());
    }
    assert!(Request::decode(0, &[]).is_err());
    assert!(Request::decode(999, &[]).is_err());
}

#[test]
fn every_event_round_trips_and_truncation_fails() {
    for event in sample_events() {
        let frame = event.encode().unwrap();
        let (op, body) = split(&frame);
        assert_eq!(Event::decode(op, body).unwrap(), event);
        assert!(
            Event::decode(op, &body[..body.len() - 1]).is_err(),
            "{event:?}"
        );
    }
    assert!(Event::decode(0, &[]).is_err());
    assert!(Event::decode(500, &[1]).is_err());
}

#[test]
fn limits_are_enforced_when_decoding() {
    // 33 damage rectangles.
    let mut body = 1u32.to_le_bytes().to_vec();
    body.push(33);
    body.extend(std::iter::repeat_n(0u8, 33 * 16));
    assert!(Request::decode(11, &body).is_err());
    // 9 MIME types.
    let mut body = 1u32.to_le_bytes().to_vec();
    body.push(9);
    assert!(Request::decode(18, &body).is_err());
    // Unknown role, edge, state, format.
    assert!(Request::decode(2, &[1, 0, 0, 0, 7]).is_err());
    assert!(Request::decode(2, &[1, 0, 0, 0, 2, 4, 0, 0, 0, 0]).is_err());
    assert!(Request::decode(14, &[1, 0, 0, 0, 4]).is_err());
    assert!(Request::decode(7, &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 2]).is_err());
}

#[test]
fn rights_and_roles() {
    assert!(Rights::ALL.contains(Rights::CAPTURE));
    assert!(!Rights::SHELL.contains(Rights::CAPTURE));
    assert_eq!(Rights::SHELL.union(Rights::CAPTURE).0, 9);
    assert_eq!(Role::Toplevel.required_rights(), Rights::NONE);
    assert_eq!(Role::Lock.required_rights(), Rights::LOCK_SCREEN);
    assert_eq!(Role::InputPanel.required_rights(), Rights::INPUT_METHOD);
    assert_eq!(Role::Overlay.required_rights(), Rights::OVERLAY);
    assert_eq!(
        Role::Panel {
            edge: Edge::Top,
            size: 1
        }
        .required_rights(),
        Rights::SHELL
    );
    assert!(Modifiers(Modifiers::CTRL | Modifiers::SUPER).has(Modifiers::SUPER));
}
