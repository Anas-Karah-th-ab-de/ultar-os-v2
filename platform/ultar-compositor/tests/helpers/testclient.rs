//! Test client: a separate process that connects to a compositor and does
//! one scripted thing. Prints one line per interesting event on stdout so
//! the tests can follow it. Not shipped.
//!
//! `ultar-compositor-testclient SOCKET MODE [ARG...]`
//! modes: `solid RRGGBB TITLE` · `ui TITLE [info|success]` · `steal ID` · `capture` ·
//! `flood` · `noread` · `midframe` ·
//! `kerncheck COMPOSITOR [--stay]` (whole scenario on the Ultar kernel, see
//! `tools/kern-test/compositor-qemu.py`)

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;
use ultar_surface::client::{Client, ClientError, Window, WindowConfig};
use ultar_surface::proto::{BufferId, Event, OutputId, PixelFormat, Rect, Request, SurfaceId};
use ultar_surface::ui::UiWindow;

#[path = "kerncheck.rs"]
mod kerncheck;

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(socket), Some(mode)) = (args.first(), args.get(1)) else {
        say("usage");
        std::process::exit(2);
    };
    let rest = &args[2..];
    let result = match mode.as_str() {
        "solid" => solid(socket, rest),
        "ui" => ui(socket, rest),
        "steal" => steal(socket, rest),
        "capture" => capture(socket),
        "flood" => flood(socket),
        "noread" => noread(socket),
        "midframe" => midframe(socket),
        "kerncheck" => kerncheck::run(socket, rest),
        "spawncheck" => kerncheck::spawn_check(rest),
        _ => Err(ClientError::InvalidSize),
    };
    match result {
        Ok(()) => say("done"),
        Err(e) => {
            eprintln!("testclient {mode}: error {e:?}");
            say(&format!("error {e:?}"));
            std::process::exit(1);
        }
    }
}

fn connect(socket: &str, app: &str) -> Result<Client, ClientError> {
    Client::connect(Path::new(socket), app)
}

fn fill(color: u32, w: u32, h: u32) -> Vec<u8> {
    let px = (color | 0xff00_0000).to_le_bytes();
    px.iter()
        .copied()
        .cycle()
        .take(w as usize * h as usize * 4)
        .collect()
}

fn describe(event: &Event) -> Option<String> {
    Some(match event {
        Event::Key {
            code,
            state,
            character,
            ..
        } => format!("key {code} {state} {character}"),
        Event::PointerButton {
            button, pressed, ..
        } => format!("button {button} {}", u8::from(*pressed)),
        Event::PointerEnter { .. } => "enter".into(),
        Event::PointerLeave { .. } => "leave".into(),
        Event::KeyboardFocus { focused, .. } => format!("focus {}", u8::from(*focused)),
        Event::Text { text, .. } => format!("text {text}"),
        Event::TouchDown { id, .. } => format!("touch {id}"),
        Event::SelectionOffer { mimes } => format!("offer {}", mimes.len()),
        _ => return None,
    })
}

fn solid(socket: &str, args: &[String]) -> Result<(), ClientError> {
    // A compositor restart ends the connection: connect again and show the
    // window again (apps keep their state, the compositor does not).
    loop {
        match solid_once(socket, args) {
            Err(ClientError::Disconnected) => {
                say("reconnecting");
                std::thread::sleep(Duration::from_millis(50));
            }
            other => return other,
        }
    }
}

fn solid_once(socket: &str, args: &[String]) -> Result<(), ClientError> {
    let color = args
        .first()
        .and_then(|c| u32::from_str_radix(c, 16).ok())
        .unwrap_or(0x808080);
    let title = args.get(1).cloned().unwrap_or_default();
    let mut client = Client::connect_retry(
        Path::new(socket),
        "org.ultar.test.solid",
        200,
        Duration::from_millis(50),
    )?;
    let mut window = Window::create(&mut client, &WindowConfig::toplevel(&title))?;
    let mut drawn: Option<(u32, u32)> = None;
    loop {
        let Some(event) = client.next_event(None)? else {
            continue;
        };
        if window.handle(&event) {
            if window.close_requested() {
                say("close");
                return Ok(());
            }
        } else if let Some(line) = describe(&event) {
            say(&line);
        }
        if let Some(c) = window.configure() {
            let size = ultar_surface::ui::device_size(&c);
            if drawn != Some(size) && window.can_draw() {
                window.present(&mut client, &fill(color, size.0, size.1), size, &[])?;
                drawn = Some(size);
                say(&format!("drawn {} {}", size.0, size.1));
            }
        }
    }
}

fn ui(socket: &str, args: &[String]) -> Result<(), ClientError> {
    use ultar_ui::event::ActionKind;
    use ultar_ui::paint::PaintEnv;
    use ultar_ui::tokens::{InputMode, ThemeKind};
    use ultar_ui::widgets::{Button, Card, Label, Switch};
    let title = args.first().cloned().unwrap_or_default();
    let client = connect(socket, "org.ultar.test.ui")?;
    let lang = ultar_ui::Lang::En;
    let tr = ultar_i18n::Translator::new(lang);
    let env = PaintEnv::new(lang, ThemeKind::Dark);
    let mut ui = ultar_ui::ui::Ui::new(
        env,
        InputMode::Pointer,
        ultar_ui::Size::new(400.0, 300.0),
        tr,
    )
    .map_err(|_| ClientError::InvalidSize)?;
    let background = match args.get(1).map(String::as_str) {
        Some("info") => ultar_ui::tokens::ColorRole::Info,
        Some("success") => ultar_ui::tokens::ColorRole::Success,
        _ => ultar_ui::tokens::ColorRole::Surface,
    };
    ui.set_background(background);
    let root = ui.root();
    let card = ui
        .add(root, Card::new())
        .map_err(|_| ClientError::InvalidSize)?;
    let button = ui
        .add(card, Button::new(Label::data("OK 42")))
        .map_err(|_| ClientError::InvalidSize)?;
    ui.add(card, Switch::new(Label::data("Wi-Fi"), true))
        .map_err(|_| ClientError::InvalidSize)?;
    let mut win = UiWindow::new(client, &WindowConfig::toplevel(&title), ui)?;
    let mut announced = false;
    loop {
        for action in win.dispatch(Some(Duration::from_millis(500)))? {
            if action.node == button && action.kind == ActionKind::Activated {
                say("activated");
            }
        }
        if !announced && !win.window().can_draw() {
            announced = true;
            say("drawn");
        }
        if win.window().close_requested() {
            say("close");
            return Ok(());
        }
    }
}

fn steal(socket: &str, args: &[String]) -> Result<(), ClientError> {
    let id = args.first().and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut client = connect(socket, "org.ultar.test.steal")?;
    // The compositor may close the connection after the first: keep going
    // and read the error it sent.
    let _ = client.send(&Request::Attach {
        surface: SurfaceId(id),
        buffer: BufferId(0),
    });
    let _ = client.send(&Request::Commit {
        surface: SurfaceId(id),
        configure: 0,
    });
    wait_refusal(&mut client)
}

fn wait_refusal(client: &mut Client) -> Result<(), ClientError> {
    loop {
        match client.next_event(Some(Duration::from_secs(10))) {
            Ok(Some(Event::Error { code })) => {
                say(&format!("refused {code:?}"));
                return Ok(());
            }
            Ok(Some(Event::CaptureData {
                width,
                height,
                last: true,
                ..
            })) => {
                say(&format!("captured {width} {height}"));
                return Ok(());
            }
            Ok(Some(_)) => {}
            Ok(None) => return Err(ClientError::Timeout),
            Err(ClientError::Disconnected) => {
                say("disconnected");
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    }
}

fn capture(socket: &str) -> Result<(), ClientError> {
    let mut client = connect(socket, "org.ultar.test.capture")?;
    client.send(&Request::Capture {
        output: OutputId(0),
    })?;
    wait_refusal(&mut client)
}

fn flood(socket: &str) -> Result<(), ClientError> {
    let mut client = connect(socket, "org.ultar.test.flood")?;
    let window = Window::create(&mut client, &WindowConfig::toplevel("flood"))?;
    say("flooding");
    let rects = vec![Rect::new(0, 0, 8192, 8192); 32];
    for _ in 0..200_000 {
        let r = client.send(&Request::Damage {
            surface: window.surface(),
            rects: rects.clone(),
        });
        if r.is_err() {
            say("disconnected");
            return Ok(());
        }
    }
    // Then garbage: a frame header with reserved bits set.
    let _ = client.send(&Request::Frame {
        surface: window.surface(),
    });
    wait_refusal(&mut client)
}

fn noread(socket: &str) -> Result<(), ClientError> {
    let mut client = connect(socket, "org.ultar.test.noread")?;
    let mut window = Window::create(&mut client, &WindowConfig::toplevel("noread"))?;
    while window.configure().is_none() {
        if let Some(e) = client.next_event(Some(Duration::from_secs(10)))? {
            window.handle(&e);
        }
    }
    let c = window.configure().ok_or(ClientError::Timeout)?;
    let size = ultar_surface::ui::device_size(&c);
    window.present(&mut client, &fill(0x2040a0, size.0, size.1), size, &[])?;
    say("drawn");
    // Never read again.
    std::thread::sleep(Duration::from_secs(120));
    Ok(())
}

fn midframe(socket: &str) -> Result<(), ClientError> {
    let mut client = connect(socket, "org.ultar.test.midframe")?;
    let mut window = Window::create(&mut client, &WindowConfig::toplevel("midframe"))?;
    while window.configure().is_none() {
        if let Some(e) = client.next_event(Some(Duration::from_secs(10)))? {
            window.handle(&e);
        }
    }
    client.send(&Request::CreateInlineBuffer {
        buffer: BufferId(90),
        width: 64,
        height: 64,
        format: PixelFormat::Xrgb8888,
    })?;
    let frame = Request::WritePixels {
        buffer: BufferId(90),
        rect: Rect::new(0, 0, 64, 64),
        data: vec![7; 64 * 64 * 4],
    }
    .encode()?;
    say("half");
    // Write half of the frame, then die without warning.
    let fd = client
        .fd()
        .try_clone_to_owned()
        .map_err(|_| ClientError::Disconnected)?;
    let mut stream = UnixStream::from(fd);
    let _ = stream.write_all(&frame[..frame.len() / 2]);
    std::process::abort();
}
