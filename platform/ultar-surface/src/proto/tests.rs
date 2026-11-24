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
        Request::Unlock { surface: s },
        Request::RequestActivation { serial: 4 },
        Request::Activate { surface: s, token: u64::MAX - 3 },
        Request::ListWindows,
        Request::ShellFocus { surface: s },
        Request::ShellFocus { surface: SurfaceId(0) },
        Request::WindowAction { window: 7, action: WindowAction::Activate },
        Request::WindowAction { window: 7, action: WindowAction::Minimize },
        Request::WindowAction { window: 7, action: WindowAction::ToggleMaximize },
        Request::WindowAction { window: u32::MAX, action: WindowAction::Close },
        Request::SetDisplayMode { mode: DisplayModeRequest::Extend },
        Request::SetDisplayMode { mode: DisplayModeRequest::Mirror },
        Request::SetDisplayMode { mode: DisplayModeRequest::Single(OutputId(2)) },
        Request::SetKeyboardLayout { index: 1 },
        Request::WindowThumbnail { window: 7, max_width: 448, max_height: 288 },
        Request::WindowThumbnail { window: 1, max_width: MAX_THUMBNAIL_SIDE, max_height: 1 },
        Request::SwitchWorkspace { index: 2 },
        Request::MoveToWorkspace { window: 7, index: ALL_WORKSPACES },
        Request::CreateSurface { surface: s, role: Role::SystemDialog },
        Request::ConfigureOutputs { setup: Setup { serial: 3, arrangement: Arrangement::Single, provisional_ms: 15_000, outputs: vec![OutputSetup { output: OutputId(1), included: true, primary: true, mode: OutputMode { width: 800, height: 600, refresh_mhz: 60_000 }, scale_milli: 1000, position: (0, 0), transform: Transform::Rotate90 }] } },
        Request::ConfirmOutputs { serial: 3 },
        Request::IdentifyOutputs { numbers: vec![(OutputId(0), 1), (OutputId(1), 2)], ms: 5000 },
        Request::DetectOutputs,
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
        Event::ActivationToken { token: 0x1234_5678_9abc },
        Event::SessionLocked { locked: true },
        Event::WindowList { windows: vec![] },
        Event::WindowList {
            windows: vec![WindowEntry { app_id: "org.ultar.test.ui".into(), title: "نافذة".into(), mapped: true, shared_memory: true }],
        },
        Event::ShellKey { code: 125, modifiers: Modifiers(Modifiers::SUPER), serial: 9 },
        Event::WindowsChanged { windows: vec![] },
        Event::WindowsChanged {
            windows: vec![ShellWindow { id: 4, app_id: "org.ultar.files".into(), title: "الملفات".into(), state: WindowState::Minimized, focused: false, unresponsive: true, output: OutputId(1), workspace: ALL_WORKSPACES }],
        },
        Event::KeyboardLayouts { names: vec![], active: 0 },
        Event::KeyboardLayouts { names: vec!["ar".into(), "en".into()], active: 1 },
        Event::ShellFocusLost { surface: s },
        Event::WindowThumbnail { window: 3, width: 2, height: 1, data: vec![7; 8] },
        Event::WindowThumbnail { window: 3, width: 0, height: 0, data: vec![] },
        Event::Workspaces { current: 1, count: 2 },
        Event::EdgeSwipe { edge: SwipeEdge::Bottom, held: true },
        Event::EdgeSwipe { edge: SwipeEdge::Side, held: false },
        Event::OutputDetails(Box::new(OutputDetails { id: OutputId(2), connector: "eDP-1".into(), identity: OutputIdentity { manufacturer: "BOE".into(), product: 0x0abc, serial: 0, name: String::new() }, internal: true, size_mm: (344, 194), preferred: OutputMode { width: 1920, height: 1080, refresh_mhz: 60_000 }, current: OutputMode { width: 1920, height: 1080, refresh_mhz: 60_000 }, modes: vec![OutputMode { width: 1920, height: 1080, refresh_mhz: 60_000 }] })),
        Event::OutputSetupDone { serial: 3, outcome: SetupOutcome::Refused(SetupRefusal::Busy) },
        Event::OutputSetupDone { serial: 4, outcome: SetupOutcome::RevertedTimeout },
        Event::OutputsDetected { count: 2 },
    ]
}

fn split(frame: &[u8]) -> (u16, &[u8]) {
    (u16::from_le_bytes([frame[4], frame[5]]), &frame[HEADER..])
}

#[test]
fn every_request_round_trips_and_truncation_fails() {
    let phone = [
        Request::SetPhoneMode { on: true },
        Request::WindowAction {
            window: 3,
            action: WindowAction::Back,
        },
    ];
    for request in sample_requests().into_iter().chain(phone) {
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
    // Unknown role (8 is the call screen), edge, state, format.
    assert!(Request::decode(2, &[1, 0, 0, 0, 8]).is_ok());
    assert!(Request::decode(2, &[1, 0, 0, 0, 9]).is_err());
    assert!(Request::decode(2, &[1, 0, 0, 0, 2, 4, 0, 0, 0, 0]).is_err());
    assert!(Request::decode(14, &[1, 0, 0, 0, 4]).is_err());
    assert!(Request::decode(7, &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 2]).is_err());
}

#[test]
fn shell_messages_refuse_bad_values() {
    // Unknown window action and display mode.
    assert!(Request::decode(31, &[1, 0, 0, 0, 5]).is_err());
    assert!(Event::decode(41, &[2, 0]).is_err(), "unknown edge");
    assert!(Request::decode(41, &[2]).is_err(), "not a bool");
    assert!(Request::decode(32, &[3, 0, 0, 0, 0]).is_err());
    // 65 shell windows, 9 layouts, active layout out of range.
    assert!(Event::decode(33, &[65]).is_err());
    assert!(Event::decode(34, &[9]).is_err());
    assert!(Event::decode(34, &[1, 2, 0, b'a', b'r', 1]).is_err());
    // A list longer than the limit is cut when encoding.
    let many: Vec<ShellWindow> = (0..70)
        .map(|i| ShellWindow {
            workspace: 0,
            id: i,
            app_id: String::new(),
            title: String::new(),
            state: WindowState::Normal,
            focused: false,
            unresponsive: false,
            output: OutputId(0),
        })
        .collect();
    let frame = Event::WindowsChanged { windows: many }.encode().unwrap();
    let (op, body) = split(&frame);
    match Event::decode(op, body).unwrap() {
        Event::WindowsChanged { windows } => assert_eq!(windows.len(), MAX_SHELL_WINDOWS),
        other => panic!("{other:?}"),
    }
    assert!(Modifiers(Modifiers::CAPS_LOCK).has(Modifiers::CAPS_LOCK));
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
    assert_eq!(Role::SystemDialog.required_rights(), Rights::SYSTEM_DIALOG);
    assert_eq!(Role::CallScreen.required_rights(), Rights::CALL_SCREEN);
    assert!(Rights::ALL.contains(Rights::CALL_SCREEN));
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

#[test]
fn thumbnails_and_workspaces_out_of_bounds_are_refused() {
    let request = |max_width: u16| Request::WindowThumbnail {
        window: 1,
        max_width,
        max_height: 8,
    };
    let big = request(MAX_THUMBNAIL_SIDE + 1).encode().unwrap();
    assert!(
        Request::decode(38, &big[HEADER..]).is_err(),
        "side above 512"
    );
    let zero = request(0).encode().unwrap();
    assert!(Request::decode(38, &zero[HEADER..]).is_err(), "side 0");
    // pixels that do not match the size
    let bad = Event::WindowThumbnail {
        window: 1,
        width: 2,
        height: 2,
        data: vec![0; 8],
    };
    let frame = bad.encode().unwrap();
    assert!(Event::decode(39, &frame[HEADER..]).is_err());
    for (current, count) in [(0u8, 0u8), (2, 2), (0, MAX_WORKSPACES as u8 + 1)] {
        let e = Event::Workspaces { current, count };
        let frame = e.encode().unwrap();
        assert!(
            Event::decode(40, &frame[HEADER..]).is_err(),
            "{current}/{count}"
        );
    }
    let w = ShellWindow {
        id: 9,
        app_id: String::new(),
        title: String::new(),
        state: WindowState::Normal,
        focused: false,
        unresponsive: false,
        output: OutputId(0),
        workspace: MAX_WORKSPACES as u8,
    };
    let frame = Event::WindowsChanged { windows: vec![w] }.encode().unwrap();
    assert!(
        Event::decode(33, &frame[HEADER..]).is_err(),
        "workspace out of range"
    );
}
