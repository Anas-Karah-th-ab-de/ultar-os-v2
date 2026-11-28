//! The shared clipboard from window to window, as on two devices with the rules that are
//! shipped (`rootfs/etc/ultar/bus-policy.toml`): on each device a real compositor (headless,
//! in this process) with the right `ui.clipboard-bridge`, a real `ultar-mesh` with its own
//! bus and a stand-in for the notification service; an app window on each compositor.
//! Copy in the window of the phone, paste in the window of the computer – text, a picture;
//! never a password, never without the right.

mod display;

use display::{App, Display};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::mesh::{
    ClipboardRemote, Done, MeshState, PairBegin, PairJoin, Right, SetEnabled, SetRights,
};
use ultar_bus::messages::notify::{Post, PostReply};
use ultar_bus::{BusError, Call, Client, ErrorCode, Text, TextArg};
use ultar_surface::proto::{CLIP_IMAGE_MIME, CLIP_TEXT_MIME};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

type Notices = Arc<Mutex<Vec<Post>>>;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn call<C: Call>(client: &Client, c: &C, wait: u64) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(wait));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// One device: bus, mesh, compositor, an app window, the notices it got.
struct Device {
    _world: World,
    _mesh: Daemon,
    shell: Client,
    display: Display,
    app: App,
    notices: Notices,
}

fn device(tag: &str, name: &str, kind: &str, ports: (u16, u16)) -> Device {
    let policy = std::fs::read_to_string(repo().join("rootfs/etc/ultar/bus-policy.toml")).unwrap();
    let world = World::new(
        tag,
        &policy,
        &["system.mesh", "system.shell", "system.notifyd"],
    );
    let notices = Notices::default();
    let seen = notices.clone();
    world.serve("system.notifyd", "notify", move |request| {
        if let Ok(post) = request.decode::<Post>() {
            seen.lock().unwrap().push(post);
        }
        let _ = request.reply(&PostReply { id: 1 });
    });
    let socket = world.dir.join("compositor.sock");
    let display = Display::start(&socket);
    let (state, inbox) = (world.subdir("state"), world.subdir("inbox"));
    let (port, peer) = (ports.0.to_string(), format!("127.0.0.1:{}", ports.1));
    let args = [
        ["--state", state.to_str().unwrap()],
        ["--inbox", inbox.to_str().unwrap()],
        ["--name", name],
        ["--kind", kind],
        ["--port", &port],
        ["--announce", "off"],
        ["--peer", &peer],
        ["--compositor", socket.to_str().unwrap()],
    ];
    let mesh = world.spawn(
        env!("CARGO_BIN_EXE_ultar-mesh"),
        "system.mesh",
        &args.concat(),
    );
    let shell = world.client("system.shell");
    let app = App::open(&display);
    Device {
        _world: world,
        _mesh: mesh,
        shell,
        display,
        app,
        notices,
    }
}

impl Device {
    fn mesh(&self) -> MeshState {
        let states = self.shell.subscribe::<MeshState>().unwrap();
        states.recv_timeout(WAIT).unwrap()
    }

    fn peer_called(&self, name: &str, paired: bool) -> String {
        let mut id = None;
        assert!(wait_until(WAIT, || {
            let state = self.mesh();
            id = state
                .peers
                .iter()
                .find(|p| p.name == name && p.paired == paired)
                .map(|p| p.id.clone());
            id.is_some()
        }));
        id.unwrap()
    }

    /// "Share the clipboard" for `peer`, as the Settings app sets it.
    fn share(&self, peer: &str, on: bool) {
        let mut rights = vec![Right::Handoff, Right::Files];
        if on {
            rights.push(Right::Clipboard);
        }
        let set = SetRights::of(peer, rights);
        assert_eq!(call(&self.shell, &set, 10).unwrap(), Done { ok: true });
    }
}

/// Both on and paired; the sharing is still off (the default).
fn pair(phone: &Device, desk: &Device) -> (String, String) {
    let on = SetEnabled { enabled: true };
    assert_eq!(call(&desk.shell, &on, 10).unwrap(), Done { ok: true });
    assert_eq!(call(&phone.shell, &on, 10).unwrap(), Done { ok: true });
    let desk_id = phone.peer_called("Desk", false);
    let code = call(&desk.shell, &PairBegin, 10).unwrap().code;
    let join = PairJoin {
        device: desk_id.clone(),
        code,
    };
    assert_eq!(call(&phone.shell, &join, 30).unwrap(), Done { ok: true });
    let phone_id = desk.peer_called("Phone", true);
    (phone_id, desk_id)
}

/// Waits until the two devices are linked and know what each other can do.
fn linked(phone: &Device, desk: &Device) {
    for device in [phone, desk] {
        assert!(wait_until(WAIT, || {
            device
                .mesh()
                .peers
                .iter()
                .any(|p| p.paired && p.reachable && !p.capabilities.is_empty())
        }));
    }
}

/// Copies in the phone's window until the computer's window can paste it (the bridge of a
/// device that was switched on a moment ago may still be connecting).
fn arrives(phone: &mut Device, desk: &mut Device, mime: &str, data: &[u8]) -> bool {
    wait_until(WAIT, || {
        phone.app.copy(&phone.display, mime, data);
        wait_until(Duration::from_millis(2500), || {
            let got = desk.app.paste(&desk.display, mime);
            if got.as_deref() != Some(data) {
                std::thread::sleep(Duration::from_millis(100));
                return false;
            }
            true
        })
    })
}

type Arrived = ultar_bus::TypedSubscription<ClipboardRemote>;

/// 1. a new pairing shares nothing: the default is off
fn step_default_off(phone: &mut Device, desk: &mut Device, arrived: &Arrived) {
    phone.app.copy(&phone.display, CLIP_TEXT_MIME, b"not yet");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(desk.app.paste(&desk.display, CLIP_TEXT_MIME), None);
    assert!(
        arrived.try_recv().ok().flatten().is_none(),
        "nothing arrived"
    );
}

/// 2. the person switches "Share the clipboard" on, on both devices
fn step_text_and_notice(phone: &mut Device, desk: &mut Device, ids: (&str, &str)) {
    let (phone_id, desk_id) = ids;
    phone.share(desk_id, true);
    desk.share(phone_id, true);
    assert!(arrives(
        phone,
        desk,
        CLIP_TEXT_MIME,
        "Grüße – مرحبا".as_bytes()
    ));
    assert!(wait_until(WAIT, || !desk
        .notices
        .lock()
        .unwrap()
        .is_empty()));
    let notice = desk.notices.lock().unwrap()[0].clone();
    assert_eq!(notice.title.key, "mesh.clip.title");
    assert_eq!(notice.title.args, vec![TextArg::Verbatim("Phone".into())]);
    assert_eq!(
        notice.body,
        ultar_bus::messages::notify::Body::Localized(Text::key("mesh.clip.body"))
    );
    assert!(
        phone.notices.lock().unwrap().is_empty(),
        "the sender is not told"
    );
}

/// 3. a picture
fn step_picture(phone: &mut Device, desk: &mut Device) {
    let mut picture = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    picture.extend((0..300_000).map(|i| (i % 249) as u8));
    assert!(arrives(phone, desk, CLIP_IMAGE_MIME, &picture));
}

/// 4. a password never leaves the phone
fn step_password(phone: &mut Device, desk: &mut Device, arrived: &Arrived) {
    phone.app.password_field(true);
    phone.app.copy(&phone.display, CLIP_TEXT_MIME, b"hunter2");
    phone.app.password_field(false);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(arrives(phone, desk, CLIP_TEXT_MIME, b"after the password"));
    let mut texts = Vec::new();
    while let Ok(Some(c)) = arrived.try_recv() {
        texts.push(c.text);
    }
    assert!(!texts.is_empty());
    assert!(
        texts.iter().all(|t| t != "hunter2"),
        "the computer got: {texts:?}"
    );
}

/// 5. the computer takes the right back: what the phone copies is dropped there
fn step_revoked(phone: &mut Device, desk: &mut Device, phone_id: &str) {
    desk.share(phone_id, false);
    std::thread::sleep(Duration::from_millis(300));
    phone.app.copy(&phone.display, CLIP_TEXT_MIME, b"dropped");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        desk.app.paste(&desk.display, CLIP_TEXT_MIME).as_deref(),
        Some(&b"after the password"[..])
    );
    desk.share(phone_id, true);
    assert!(arrives(phone, desk, CLIP_TEXT_MIME, b"back again"));
}

#[test]
fn copy_on_the_phone_paste_on_the_computer() {
    let (p, d) = (free_port(), free_port());
    let mut phone = device("clip-e2e-phone", "Phone", "phone", (p, d));
    let mut desk = device("clip-e2e-desk", "Desk", "desktop", (d, p));
    let arrived = desk.shell.subscribe::<ClipboardRemote>().unwrap();
    let (phone_id, desk_id) = pair(&phone, &desk);
    linked(&phone, &desk);
    step_default_off(&mut phone, &mut desk, &arrived);
    step_text_and_notice(&mut phone, &mut desk, (&phone_id, &desk_id));
    step_picture(&mut phone, &mut desk);
    step_password(&mut phone, &mut desk, &arrived);
    step_revoked(&mut phone, &mut desk, &phone_id);
    assert!(arrives(
        &mut desk,
        &mut phone,
        CLIP_TEXT_MIME,
        "من الحاسوب".as_bytes()
    ));
}
