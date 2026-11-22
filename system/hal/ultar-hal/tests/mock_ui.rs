//! Exercises the display, renderer, input, audio, camera, haptics and LED traits
//! through the mock backend, including error paths and scripted failures.

use ultar_hal::mock::MockBackend;
use ultar_hal::{
    AudioDirection, AudioEndpointKind, AudioFormat, Backend, CameraFacing, CameraId, DisplayEvent,
    DisplayId, DisplayMode, EndpointId, FrameView, HalError, InputDeviceId, InputDeviceKind,
    InputEventKind, KeyState, LedId, LedState, Level, PixelFormat, PointerButton, Rect,
    RendererKind, Rotation, SampleFormat, Size,
};

fn frame_bytes(mode: DisplayMode) -> Vec<u8> {
    vec![0; (mode.size.area() * 4) as usize]
}

#[test]
fn display_modes_and_presenting_with_damage() {
    let laptop = MockBackend::laptop();
    let display = laptop.display().unwrap();
    let info = display.displays().unwrap().remove(0);
    assert!(info.internal);
    assert_eq!(info.connector, "eDP-1");
    let modes = display.modes(info.id).unwrap();
    assert_eq!(modes.len(), 2);
    assert_eq!(display.current_mode(info.id).unwrap(), modes[0]);

    let pixels = frame_bytes(modes[0]);
    let stride = modes[0].size.width * 4;
    let frame = FrameView::new(modes[0].size, stride, PixelFormat::Xrgb8888, &pixels).unwrap();
    display.present(info.id, &frame, &[]).unwrap();
    assert_eq!(
        laptop.ui.display.last_damage(info.id),
        vec![Rect::from_size(modes[0].size)]
    );
    let damage = [Rect::new(-10, 0, 20, 20), Rect::new(5000, 0, 5, 5)];
    display.present(info.id, &frame, &damage).unwrap();
    assert_eq!(
        laptop.ui.display.last_damage(info.id),
        vec![Rect::new(0, 0, 10, 20)]
    );
    assert_eq!(laptop.ui.display.presented_frames(info.id), 2);

    // After a mode switch the old frame size is refused.
    display.set_mode(info.id, modes[1]).unwrap();
    assert_eq!(
        display.present(info.id, &frame, &[]),
        Err(HalError::InvalidArgument)
    );
    assert_eq!(
        display.set_mode(info.id, DisplayMode::new(123, 45, 60)),
        Err(HalError::InvalidArgument)
    );
}

#[test]
fn display_brightness_rotation_and_unknown_ids() {
    let phone = MockBackend::phone();
    let display = phone.display().unwrap();
    let id = display.displays().unwrap()[0].id;
    assert_eq!(display.brightness(id).unwrap(), Level::MAX);
    let dim = Level::from_percent(30).unwrap();
    display.set_brightness(id, dim).unwrap();
    assert_eq!(display.brightness(id).unwrap(), dim);
    assert_eq!(display.rotation(id).unwrap(), Rotation::Deg0);
    display.set_rotation(id, Rotation::Deg270).unwrap();
    assert_eq!(display.rotation(id).unwrap(), Rotation::Deg270);

    let missing = DisplayId(99);
    assert_eq!(display.modes(missing), Err(HalError::NotFound));
    assert_eq!(display.current_mode(missing), Err(HalError::NotFound));
    assert_eq!(display.brightness(missing), Err(HalError::NotFound));
    assert_eq!(
        display.set_brightness(missing, dim),
        Err(HalError::NotFound)
    );
    assert_eq!(display.rotation(missing), Err(HalError::NotFound));
    assert_eq!(
        display.set_rotation(missing, Rotation::Deg90),
        Err(HalError::NotFound)
    );
    assert_eq!(phone.ui.display.presented_frames(missing), 0);
    assert!(phone.ui.display.last_damage(missing).is_empty());
}

#[test]
fn display_hotplug_events() {
    let phone = MockBackend::phone();
    let display = phone.display().unwrap();
    let internal = display.displays().unwrap()[0].id;
    assert_eq!(
        display.poll_events().unwrap(),
        vec![DisplayEvent::Connected(internal)]
    );
    assert!(display.poll_events().unwrap().is_empty());

    let modes = [
        DisplayMode::new(3840, 2160, 60),
        DisplayMode::new(1920, 1080, 60),
    ];
    let monitor = phone
        .ui
        .display
        .plug("DP-1", false, Size::new(597, 336), &modes);
    display.set_mode(monitor, modes[1]).unwrap();
    display.set_mode(monitor, modes[1]).unwrap();
    assert_eq!(
        display.poll_events().unwrap(),
        vec![
            DisplayEvent::Connected(monitor),
            DisplayEvent::ModeChanged(monitor)
        ]
    );
    assert_eq!(display.displays().unwrap().len(), 2);
    phone.ui.display.unplug(monitor).unwrap();
    assert_eq!(phone.ui.display.unplug(monitor), Err(HalError::NotFound));
    assert_eq!(
        display.poll_events().unwrap(),
        vec![DisplayEvent::Disconnected(monitor)]
    );
    assert_eq!(display.current_mode(monitor), Err(HalError::NotFound));

    // A display plugged without modes gets a fallback mode.
    let bare = phone.ui.display.plug("VGA-1", false, Size::default(), &[]);
    assert_eq!(
        display.current_mode(bare).unwrap(),
        DisplayMode::new(1024, 768, 60)
    );
}

#[test]
fn display_scripted_failure_hits_one_call() {
    let phone = MockBackend::phone();
    let display = phone.display().unwrap();
    phone.ui.display.fail_next(HalError::Io(5));
    assert_eq!(display.displays(), Err(HalError::Io(5)));
    assert_eq!(display.displays().unwrap().len(), 1);
}

#[test]
fn renderer_kinds() {
    let backend = MockBackend::full("r");
    let renderer = backend.renderer().unwrap();
    assert_eq!(renderer.kind(), RendererKind::Software);
    assert!(renderer.supports_format(PixelFormat::Argb8888));
    assert!(!renderer.supports_format(PixelFormat::Rgb565));
    let software_limit = renderer.max_surface_size();
    backend.ui.renderer.set_gpu(true);
    assert_eq!(renderer.kind(), RendererKind::Gpu);
    assert!(renderer.supports_format(PixelFormat::Rgb565));
    assert!(!renderer.supports_format(PixelFormat::Mjpeg));
    assert!(renderer.max_surface_size() > software_limit);
}

#[test]
fn input_events_arrive_in_order_with_rising_time() {
    let phone = MockBackend::phone();
    let input = phone.input().unwrap();
    let devices = input.devices().unwrap();
    assert_eq!(devices.len(), 2);
    let touch = devices[0].id;
    assert_eq!(devices[0].kind, InputDeviceKind::Touchscreen);
    // The two hotplug events of the preset come first.
    let hotplug = input.poll(0).unwrap();
    assert!(hotplug
        .iter()
        .all(|event| event.kind == InputEventKind::DeviceAdded));
    assert_eq!(hotplug.len(), 2);

    let mock = &phone.ui.input;
    mock.inject_tap(touch, 30_000, 40_000).unwrap();
    let second_finger = InputEventKind::TouchDown {
        slot: 1,
        x: 1,
        y: 2,
    };
    mock.inject(touch, second_finger).unwrap();
    mock.inject(
        touch,
        InputEventKind::TouchMove {
            slot: 1,
            x: 3,
            y: 4,
        },
    )
    .unwrap();
    mock.inject(touch, InputEventKind::TouchCancel).unwrap();
    assert_eq!(mock.pending(), 5);
    let events = input.poll(100).unwrap();
    assert_eq!(events.len(), 5);
    assert_eq!(
        events[0].kind,
        InputEventKind::TouchDown {
            slot: 0,
            x: 30_000,
            y: 40_000
        }
    );
    assert_eq!(events[1].kind, InputEventKind::TouchUp { slot: 0 });
    assert_eq!(events[2].kind, second_finger);
    assert_eq!(events[4].kind, InputEventKind::TouchCancel);
    assert!(events
        .windows(2)
        .all(|pair| pair[0].time_us < pair[1].time_us));
    assert!(events.iter().all(|event| event.device == touch));
    assert!(input.poll(0).unwrap().is_empty());
}

#[test]
fn input_keyboard_pointer_and_hotplug() {
    let phone = MockBackend::phone();
    let input = phone.input().unwrap();
    let mock = &phone.ui.input;
    input.poll(0).unwrap();

    let keyboard = mock.add_device(InputDeviceKind::Keyboard, "usb-keyboard", false);
    let mouse = mock.add_device(InputDeviceKind::Mouse, "usb-mouse", false);
    mock.inject_key(keyboard, 30).unwrap();
    mock.inject(mouse, InputEventKind::PointerMotion { dx: 2.5, dy: -1.0 })
        .unwrap();
    mock.inject(mouse, InputEventKind::PointerAbsolute { x: 100, y: 200 })
        .unwrap();
    let click = InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: true,
    };
    mock.inject(mouse, click).unwrap();
    mock.inject(mouse, InputEventKind::Scroll { dx: 0.0, dy: 1.0 })
        .unwrap();
    mock.remove_device(mouse).unwrap();

    let kinds: Vec<InputEventKind> = input.poll(0).unwrap().iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            InputEventKind::DeviceAdded,
            InputEventKind::DeviceAdded,
            InputEventKind::Key {
                code: 30,
                state: KeyState::Pressed
            },
            InputEventKind::Key {
                code: 30,
                state: KeyState::Released
            },
            InputEventKind::PointerMotion { dx: 2.5, dy: -1.0 },
            InputEventKind::PointerAbsolute { x: 100, y: 200 },
            click,
            InputEventKind::Scroll { dx: 0.0, dy: 1.0 },
            InputEventKind::DeviceRemoved,
        ]
    );
    assert_eq!(input.devices().unwrap().len(), 3);
    // A removed or unknown device cannot send events.
    assert_eq!(mock.inject_key(mouse, 1), Err(HalError::NotFound));
    assert_eq!(mock.remove_device(mouse), Err(HalError::NotFound));
    assert_eq!(
        mock.inject_tap(InputDeviceId(77), 0, 0),
        Err(HalError::NotFound)
    );
}

#[test]
fn input_scripted_failure_keeps_the_queue() {
    let phone = MockBackend::phone();
    let input = phone.input().unwrap();
    phone.ui.input.fail_next(HalError::Disconnected);
    assert_eq!(input.poll(0), Err(HalError::Disconnected));
    assert_eq!(input.poll(0).unwrap().len(), 2);
}

#[test]
fn audio_endpoints_volume_mute_and_route() {
    let phone = MockBackend::phone();
    let audio = phone.audio().unwrap();
    let outputs = audio.outputs().unwrap();
    let inputs = audio.inputs().unwrap();
    assert_eq!((outputs.len(), inputs.len()), (2, 1));
    let speaker = outputs[0].id;
    let earpiece = outputs[1].id;
    assert_eq!(outputs[1].kind, AudioEndpointKind::Earpiece);

    assert_eq!(audio.volume(speaker).unwrap().percent(), 50);
    audio.set_volume(speaker, Level::MAX).unwrap();
    assert_eq!(audio.volume(speaker).unwrap(), Level::MAX);
    assert!(!audio.is_muted(speaker).unwrap());
    audio.set_muted(speaker, true).unwrap();
    assert!(audio.is_muted(speaker).unwrap());

    assert_eq!(audio.route(AudioDirection::Output).unwrap(), speaker);
    assert_eq!(audio.route(AudioDirection::Input).unwrap(), inputs[0].id);
    audio.set_route(earpiece).unwrap();
    assert_eq!(audio.route(AudioDirection::Output).unwrap(), earpiece);
    phone.ui.audio.set_available(speaker, false).unwrap();
    assert_eq!(audio.set_route(speaker), Err(HalError::Disconnected));

    let missing = EndpointId(42);
    assert_eq!(audio.volume(missing), Err(HalError::NotFound));
    assert_eq!(
        audio.set_volume(missing, Level::MIN),
        Err(HalError::NotFound)
    );
    assert_eq!(audio.is_muted(missing), Err(HalError::NotFound));
    assert_eq!(audio.set_muted(missing, true), Err(HalError::NotFound));
    assert_eq!(audio.set_route(missing), Err(HalError::NotFound));
    assert_eq!(
        phone.ui.audio.set_available(missing, true),
        Err(HalError::NotFound)
    );
    phone.ui.audio.fail_next(HalError::Busy);
    assert_eq!(audio.outputs(), Err(HalError::Busy));
}

#[test]
fn audio_streams_play_and_record_whole_frames() {
    let phone = MockBackend::phone();
    let audio = phone.audio().unwrap();
    let speaker = audio.outputs().unwrap()[0].id;
    let microphone = audio.inputs().unwrap()[0].id;

    let mut playback = audio.open_stream(speaker, AudioFormat::DEFAULT).unwrap();
    assert_eq!(playback.direction(), AudioDirection::Output);
    assert_eq!(playback.format(), AudioFormat::DEFAULT);
    // 10 bytes hold two whole stereo 16 bit frames.
    assert_eq!(playback.write(&[0; 10]).unwrap(), 8);
    assert_eq!(playback.read(&mut [0; 8]), Err(HalError::NotSupported));
    playback.drain().unwrap();
    assert_eq!(phone.ui.audio.bytes_played(), 8);

    let mono = AudioFormat {
        sample_rate: 16_000,
        channels: 1,
        sample: SampleFormat::S16,
    };
    let mut capture = audio.open_stream(microphone, mono).unwrap();
    let mut buffer = [0xff_u8; 5];
    assert_eq!(capture.read(&mut buffer).unwrap(), 4);
    assert_eq!(buffer, [0, 1, 2, 3, 0xff]);
    assert_eq!(capture.read(&mut buffer[..2]).unwrap(), 2);
    assert_eq!(buffer[..2], [4, 5]);
    assert_eq!(capture.write(&[0; 4]), Err(HalError::NotSupported));
    assert_eq!(phone.ui.audio.bytes_recorded(), 6);

    assert_eq!(phone.ui.audio.open_streams(), 2);
    drop(playback);
    drop(capture);
    assert_eq!(phone.ui.audio.open_streams(), 0);
}

#[test]
fn audio_open_stream_errors() {
    let phone = MockBackend::phone();
    let audio = phone.audio().unwrap();
    let speaker = audio.outputs().unwrap()[0].id;
    let silent = AudioFormat {
        channels: 0,
        ..AudioFormat::DEFAULT
    };
    let open = |endpoint, format| audio.open_stream(endpoint, format).err();
    assert_eq!(open(speaker, silent), Some(HalError::InvalidArgument));
    assert_eq!(
        open(EndpointId(9), AudioFormat::DEFAULT),
        Some(HalError::NotFound)
    );
    phone.ui.audio.set_available(speaker, false).unwrap();
    assert_eq!(
        open(speaker, AudioFormat::DEFAULT),
        Some(HalError::Disconnected)
    );
    assert_eq!(phone.ui.audio.open_streams(), 0);
    // Without any endpoint there is no route.
    let empty = MockBackend::full("no-audio");
    assert_eq!(
        empty.audio().unwrap().route(AudioDirection::Output),
        Err(HalError::NotFound)
    );
}

#[test]
fn camera_streams_and_torch() {
    let phone = MockBackend::phone();
    let camera = phone.camera().unwrap();
    let cameras = camera.list().unwrap();
    assert_eq!(cameras.len(), 2);
    let (back, front) = (cameras[0], cameras[1]);
    assert_eq!(back.facing, CameraFacing::Back);
    assert_eq!(front.facing, CameraFacing::Front);
    let formats = camera.formats(back.id).unwrap();
    assert_eq!(formats.len(), 2);

    let mut stream = camera.open(back.id, formats[0]).unwrap();
    assert_eq!(stream.format(), formats[0]);
    let first = stream.next_frame(100).unwrap();
    let second = stream.next_frame(100).unwrap();
    assert_eq!((first.sequence, second.sequence), (0, 1));
    assert_eq!(first.data.len(), 640 * 480 * 2);
    assert!(second.data.iter().all(|byte| *byte == 1));
    assert!(second.time_us > first.time_us);
    assert_eq!(phone.ui.camera.streaming(), vec![back.id]);

    // The same camera cannot be opened twice; the other one can.
    assert_eq!(camera.open(back.id, formats[0]).err(), Some(HalError::Busy));
    let other = camera.open(front.id, formats[0]).unwrap();
    drop(stream);
    assert_eq!(phone.ui.camera.streaming(), vec![front.id]);
    drop(other);
    let mut compressed = camera.open(back.id, formats[1]).unwrap();
    assert_eq!(compressed.next_frame(0).unwrap().data.len(), 64);

    camera.set_torch(back.id, true).unwrap();
    assert!(phone.ui.camera.torch_on(back.id));
    camera.set_torch(back.id, false).unwrap();
    assert!(!phone.ui.camera.torch_on(back.id));
    assert_eq!(
        camera.set_torch(front.id, true),
        Err(HalError::NotSupported)
    );
}

#[test]
fn camera_errors() {
    let phone = MockBackend::phone();
    let camera = phone.camera().unwrap();
    let front = camera.list().unwrap()[1].id;
    let back_only = camera.formats(CameraId(0)).unwrap()[1];
    assert_eq!(
        camera.open(front, back_only).err(),
        Some(HalError::InvalidArgument)
    );
    let missing = CameraId(9);
    assert_eq!(camera.formats(missing), Err(HalError::NotFound));
    assert_eq!(
        camera.open(missing, back_only).err(),
        Some(HalError::NotFound)
    );
    assert_eq!(camera.set_torch(missing, true), Err(HalError::NotFound));
    assert!(!phone.ui.camera.torch_on(missing));
    phone.ui.camera.fail_next(HalError::Timeout);
    assert_eq!(camera.list(), Err(HalError::Timeout));
    assert!(phone.ui.camera.streaming().is_empty());
}

#[test]
fn haptics_record_pulses() {
    let phone = MockBackend::phone();
    let haptics = phone.haptics().unwrap();
    let strong = Level::MAX;
    haptics.vibrate(40, strong).unwrap();
    haptics
        .vibrate(15, Level::from_percent(20).unwrap())
        .unwrap();
    haptics.cancel().unwrap();
    assert_eq!(haptics.vibrate(0, strong), Err(HalError::InvalidArgument));
    phone.ui.haptics.fail_next(HalError::Busy);
    assert_eq!(haptics.cancel(), Err(HalError::Busy));
    let pulses = phone.ui.haptics.pulses();
    assert_eq!(pulses.len(), 2);
    assert_eq!(pulses[0], (40, strong));
    assert_eq!(phone.ui.haptics.cancels(), 1);
}

#[test]
fn leds_keep_their_state() {
    let phone = MockBackend::phone();
    let leds = phone.leds().unwrap();
    let list = leds.list().unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].has_color);
    let id = list[0].id;
    assert_eq!(leds.state(id).unwrap(), LedState::OFF);
    let blink = LedState::blinking(0, 255, 0, 300, 700);
    leds.set(id, blink).unwrap();
    assert_eq!(leds.state(id).unwrap(), blink);
    let charging = phone.ui.leds.add_led("charging", false);
    assert_eq!(leds.list().unwrap().len(), 2);
    assert_eq!(leds.state(charging).unwrap(), LedState::OFF);
    assert_eq!(leds.state(LedId(9)), Err(HalError::NotFound));
    assert_eq!(leds.set(LedId(9), blink), Err(HalError::NotFound));
    phone.ui.leds.fail_next(HalError::PermissionDenied);
    assert_eq!(leds.set(id, LedState::OFF), Err(HalError::PermissionDenied));
    assert_eq!(leds.state(id).unwrap(), blink);
}
