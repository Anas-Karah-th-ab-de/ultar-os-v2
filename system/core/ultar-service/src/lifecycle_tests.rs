use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

#[test]
fn states_only_move_forward() {
    let life = Lifecycle::new();
    assert_eq!(life.state(), State::Starting);
    assert!(life.set_ready());
    assert!(!life.set_ready());
    assert_eq!(life.state(), State::Ready);
    assert!(life.request_stop(StopCause::Request));
    assert!(!life.request_stop(StopCause::Fault));
    assert_eq!(life.stop_cause(), Some(StopCause::Request));
    assert!(!life.set_ready());
    assert_eq!(life.state(), State::Stopping);
    life.mark_stopped();
    assert_eq!(life.state(), State::Stopped);
    assert!(!life.request_stop(StopCause::Fault));
    assert!(life.is_stopping());
}

#[test]
fn a_stop_before_ready_is_possible() {
    let life = Lifecycle::default();
    assert!(life.request_stop(StopCause::Signal(Signal::Terminate)));
    assert!(!life.set_ready());
    assert_eq!(life.wait_stop(), StopCause::Signal(Signal::Terminate));
}

#[test]
fn stop_hooks_run_once_and_late_hooks_run_at_once() {
    let life = Lifecycle::new();
    let count = Arc::new(AtomicU32::new(0));
    let c = count.clone();
    assert!(life.on_stop(move || {
        c.fetch_add(1, Ordering::SeqCst);
    }));
    life.request_stop(StopCause::Request);
    life.request_stop(StopCause::Request);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let c = count.clone();
    assert!(life.on_stop(move || {
        c.fetch_add(10, Ordering::SeqCst);
    }));
    assert_eq!(count.load(Ordering::SeqCst), 11);
}

#[test]
fn waiters_wake_only_after_the_stop_hooks_ran() {
    use std::sync::mpsc::channel;
    let life = Lifecycle::new();
    let (entered, hook_entered) = channel();
    let (release, hook_may_end) = channel::<()>();
    life.on_stop(move || {
        entered.send(()).unwrap();
        hook_may_end.recv().unwrap();
    });
    let stopper = {
        let life = life.clone();
        std::thread::spawn(move || life.request_stop(StopCause::Request))
    };
    hook_entered.recv().unwrap();
    // The hook is running: the state says so, but waiters stay asleep.
    assert!(life.is_stopping());
    assert!(!life.wait_for_timeout(State::Stopping, Duration::ZERO));
    assert!(life.wait_for_timeout(State::Ready, Duration::ZERO));
    release.send(()).unwrap();
    assert_eq!(life.wait_change(0), Wake::Stop(StopCause::Request));
    assert!(stopper.join().unwrap());
}

#[test]
fn reload_hooks_run_until_the_stop() {
    let life = Lifecycle::new();
    let count = Arc::new(AtomicU32::new(0));
    let c = count.clone();
    assert!(life.on_reload(move || {
        c.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(life.reload());
    assert!(life.reload());
    assert_eq!((count.load(Ordering::SeqCst), life.reloads()), (2, 2));
    life.request_stop(StopCause::Fault);
    assert!(!life.reload());
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn hook_lists_are_bounded() {
    let life = Lifecycle::new();
    for _ in 0..MAX_HOOKS {
        assert!(life.on_stop(|| {}));
        assert!(life.on_reload(|| {}));
    }
    assert!(!life.on_stop(|| {}));
    assert!(!life.on_reload(|| {}));
}

#[test]
fn waiting_sees_the_change_from_another_thread() {
    let life = Lifecycle::new();
    assert!(!life.wait_for_timeout(State::Ready, Duration::ZERO));
    let other = life.clone();
    let waiter = std::thread::spawn(move || other.wait_stop());
    life.set_ready();
    assert!(life.wait_for_timeout(State::Ready, Duration::from_secs(30)));
    life.request_stop(StopCause::Signal(Signal::Interrupt));
    assert_eq!(waiter.join().unwrap(), StopCause::Signal(Signal::Interrupt));
    life.wait_for(State::Stopping);
}

#[test]
fn the_main_thread_can_wait_for_reloads_and_the_stop() {
    let life = Lifecycle::new();
    life.reload();
    assert_eq!(life.wait_change(0), Wake::Reload(1));
    let other = life.clone();
    let waiter = std::thread::spawn(move || other.wait_change(1));
    life.reload();
    assert_eq!(waiter.join().unwrap(), Wake::Reload(2));
    life.request_stop(StopCause::Request);
    assert_eq!(life.wait_change(0), Wake::Stop(StopCause::Request));
}

#[test]
fn names_and_numbers_are_stable() {
    for (signal, number) in [
        (Signal::Hangup, 1),
        (Signal::Interrupt, 2),
        (Signal::Terminate, 15),
    ] {
        assert_eq!(signal.number(), number);
        assert_eq!(Signal::from_number(number), Some(signal));
        assert!(!signal.name().is_empty());
    }
    assert_eq!(Signal::from_number(9), None);
    assert_eq!(State::Stopping.to_string(), "stopping");
    assert_eq!(StopCause::Request.name(), "request");
    assert_eq!(StopCause::Fault.name(), "fault");
    assert_eq!(StopCause::Signal(Signal::Hangup).name(), "hup");
}
