//! The screen service's capture against the real compositor (headless, in this process):
//! the whole output arrives, the gate opens once the picture has begun, and a client
//! without the capture right gets nothing.

use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_screend::screen::{Compositor as Capture, Screen, ScreenError};
use ultar_surface::proto::Rights;

fn compositor(path: std::path::PathBuf, stop: Arc<AtomicBool>, capture: bool) {
    std::thread::spawn(move || {
        let mut rights = RightsTable::default();
        if capture {
            rights.grant(ultar_service::effective_uid(), Rights::CAPTURE);
        }
        let config = Config {
            rights,
            cursor: false,
            ..Config::default()
        };
        let mut comp = Compositor::new(
            config,
            Box::new(Headless::new(&[(64, 48)])),
            Box::new(MonotonicClock::default()),
        );
        comp.set_listener(UnixListener::bind(&path).unwrap())
            .unwrap();
        while !stop.load(Ordering::SeqCst) {
            let _ = comp.dispatch(Some(Duration::from_millis(10)));
        }
    });
}

fn socket(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("screend-comp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("compositor.sock")
}

fn wait_for(path: &std::path::Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_whole_output_arrives_and_the_gate_opens_while_the_picture_comes() {
    let path = socket("ok");
    let stop = Arc::new(AtomicBool::new(false));
    compositor(path.clone(), stop.clone(), true);
    wait_for(&path);
    let mut screen = Capture::open(&path).unwrap();
    let mut gates = 0;
    let frame = screen.grab(&mut || gates += 1).unwrap();
    assert_eq!((frame.width, frame.height), (64, 48));
    assert_eq!(frame.data.len(), 64 * 48 * 4);
    assert_eq!(gates, 1, "opened once, as the picture began");
    // The same connection serves the next look.
    assert_eq!(screen.grab(&mut || {}).unwrap().data.len(), 64 * 48 * 4);
    stop.store(true, Ordering::SeqCst);
}

#[test]
fn without_the_capture_right_nothing_arrives_and_it_looks_like_a_lock() {
    let path = socket("denied");
    let stop = Arc::new(AtomicBool::new(false));
    compositor(path.clone(), stop.clone(), false);
    wait_for(&path);
    // The compositor shows the output only to a client with the right: no picture comes,
    // which is exactly what the service treats as a locked screen (fail safe).
    match Capture::open(&path) {
        Err(ScreenError::Gone) => {}
        Ok(mut screen) => {
            let result = screen.grab(&mut || panic!("the gate must stay shut"));
            assert!(matches!(
                result,
                Err(ScreenError::Locked | ScreenError::Gone)
            ));
        }
        Err(other) => panic!("{other:?}"),
    }
    stop.store(true, Ordering::SeqCst);
}
