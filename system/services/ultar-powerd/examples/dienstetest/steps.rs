//! The checks of the run, in order.

use crate::setup::Run;
use crate::{running, wait};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::audio::{AudioOutcome, AudioState, SetVolume};
use ultar_bus::messages::auth::{Method, Secret, SetOutcome, SetSecret};
use ultar_bus::messages::bluetooth::{BluetoothState, BtOutcome, Set as SetBluetooth};
use ultar_bus::messages::calendar::{Add, CalendarOutcome, Date, Day};
use ultar_bus::messages::lock::{Lock, LockReply};
use ultar_bus::messages::media::{Button, Command, MediaOutcome, MediaState, Next, Update};
use ultar_bus::messages::net::{NetOutcome, NetState, SetAirplane};
use ultar_bus::messages::notify::{
    Action, Body, Category, Invoke, NoteOutcome, Post, TakeInvoked, Urgency,
};
use ultar_bus::messages::power::{ActionOutcome, PowerActions, PowerOff, Suspend};
use ultar_bus::messages::session::{SessionOutcome, SessionState, SignOut, SignedIn};
use ultar_bus::messages::volumes::{Eject, EjectOutcome, VolumesState};
use ultar_bus::{Call, Client, Text, Topic};

const CALL: Duration = Duration::from_secs(30);

fn call<C: Call>(client: &Client, c: &C, what: &str) -> Result<C::Reply, String> {
    wait(what, || client.call(c, CALL).ok())
}

fn latest<T: Topic + Clone>(
    client: &Client,
    what: &str,
    ok: impl Fn(&T) -> bool,
) -> Result<T, String> {
    let sub = client
        .subscribe::<T>()
        .map_err(|e| format!("{what}: {e}"))?;
    wait(what, || {
        let mut last = None;
        while let Ok(Some(v)) = sub.try_recv() {
            last = Some(v);
        }
        last.filter(|v| ok(v))
    })
}

/// The lock screen's part: answers `lock.lock`, counts the locks.
fn lock_screen(run: &Run) -> Result<(Client, Arc<AtomicU32>), String> {
    let client = run.client("system.lock")?;
    let endpoint = client.register("lock").map_err(|e| format!("lock {e}"))?;
    let locks = Arc::new(AtomicU32::new(0));
    let count = locks.clone();
    std::thread::spawn(move || {
        endpoint.run(|r| {
            let _ = r.respond::<Lock>(|_| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(LockReply { locked: true })
            });
        })
    });
    Ok((client, locks))
}

fn sign_in(run: &Run, lock: &Client, shell: &Client) -> Result<(), String> {
    let installer = run.client("system.installer")?;
    let first = SetSecret {
        user: "amal".into(),
        display_name: "Amal".into(),
        method: Method::Password,
        current: None,
        new: Secret::new("ultar-pass-1".into()),
    };
    let made = call(&installer, &first, "account in the credential store")?;
    if made.outcome != SetOutcome::Done {
        return Err(format!("set_secret {:?}", made.outcome));
    }
    let r = call(
        lock,
        &SignedIn {
            user: "amal".into(),
        },
        "signed in",
    )?;
    if r.outcome != SessionOutcome::Done {
        return Err(format!("sign in {:?}", r.outcome));
    }
    latest::<SessionState>(shell, "session state", |s| {
        s.user.as_deref() == Some("amal")
    })?;
    println!("DIENSTE: amal signed in (credential store, sign-in screen, session state)");
    Ok(())
}

fn notifications(run: &Run, shell: &Client) -> Result<(), String> {
    let chat = run.client("app.chat")?;
    let post = Post {
        urgency: Urgency::Normal,
        category: Category::Message,
        app_id: "tech.code-craft.chat".into(),
        title: Text::key("chat.new_message"),
        body: Body::Verbatim("hello".into()),
        actions: vec![Action {
            id: "reply".into(),
            label: Text::key("chat.action.reply"),
        }],
    };
    let id = call(&chat, &post, "notify.post")?.id;
    let invoked = call(
        shell,
        &Invoke {
            id,
            action: "reply".into(),
        },
        "notify.invoke",
    )?;
    if invoked.outcome != NoteOutcome::Done {
        return Err(format!("invoke {:?}", invoked.outcome));
    }
    let back = call(&chat, &TakeInvoked {}, "take_invoked")?.invoked;
    if back.len() != 1 || back[0].id != id {
        return Err(format!("invoked {back:?}"));
    }
    println!("DIENSTE: notification button went back to its sender");
    Ok(())
}

fn calendar(run: &Run, shell: &Client) -> Result<(), String> {
    let app = run.client("app.calendar")?;
    let date = Date {
        year: 2026,
        month: 11,
        day: 24,
    };
    let add = Add {
        date,
        start_minute: 600,
        duration_minutes: 30,
        title: "doctor".into(),
    };
    // The calendar follows the session topic; it may take a moment after the sign-in.
    wait("calendar.add", || {
        app.call(&add, CALL)
            .ok()
            .filter(|r| matches!(r.outcome, CalendarOutcome::Done(_)))
    })?;
    let day = call(shell, &Day { date }, "calendar.day")?;
    if day.events.len() != 1 {
        return Err(format!("day {day:?}"));
    }
    println!("DIENSTE: calendar event of the person in front");
    Ok(())
}

fn devices(run: &Run, shell: &Client) -> Result<(), String> {
    let audio = latest::<AudioState>(shell, "audio state", |_| true)?;
    let volume = call(shell, &SetVolume { percent: 40 }, "audio.set_volume")?.outcome;
    let audio_ok = if audio.present {
        volume == AudioOutcome::Done
    } else {
        volume == AudioOutcome::NoDevice
    };
    let actions = latest::<PowerActions>(shell, "power actions", |_| true)?;
    let sleep = call(shell, &Suspend {}, "power.suspend")?.outcome;
    let sleep_ok = if actions.suspend {
        sleep == ActionOutcome::Done
    } else {
        sleep == ActionOutcome::Unsupported
    };
    if !audio_ok || !sleep_ok || (run.fake_root && !actions.suspend) {
        return Err(format!(
            "devices audio {audio:?}/{volume:?} sleep {actions:?}/{sleep:?}"
        ));
    }
    println!(
        "DIENSTE: audio present={} ({volume:?}), sleep offered={} ({sleep:?})",
        audio.present, actions.suspend
    );
    Ok(())
}

fn media(run: &Run, lock: &Client) -> Result<(), String> {
    let player = run.client("app.chat")?;
    let commands = player.subscribe::<Command>().map_err(|e| e.to_string())?;
    let up = Update {
        title: "Song".into(),
        artist: "Band".into(),
        playing: true,
        can_next: true,
        can_previous: false,
    };
    if call(&player, &up, "media.update")?.outcome != MediaOutcome::Done {
        return Err("media.update".into());
    }
    latest::<MediaState>(lock, "media card", |s| {
        s.now.as_ref().is_some_and(|n| n.player == "app.chat")
    })?;
    if call(lock, &Next {}, "media.next")?.outcome != MediaOutcome::Done {
        return Err("media.next".into());
    }
    let c = wait("media command", || commands.try_recv().ok().flatten())?;
    if c.button != Button::Next || c.player != "app.chat" {
        return Err(format!("command {c:?}"));
    }
    println!("DIENSTE: lock screen media button reached the player");
    Ok(())
}

fn radios(shell: &Client) -> Result<(), String> {
    let net = latest::<NetState>(shell, "net state", |_| true)?;
    let airplane = call(shell, &SetAirplane { on: true }, "net.set_airplane")?.outcome;
    let bt = latest::<BluetoothState>(shell, "bluetooth state", |_| true)?;
    let bt_set = call(shell, &SetBluetooth { on: true }, "bluetooth.set")?.outcome;
    let net_ok = if net.airplane_possible {
        airplane == NetOutcome::Done
    } else {
        airplane == NetOutcome::NoDevice
    };
    let bt_ok = if bt.present {
        bt_set != BtOutcome::NoDevice
    } else {
        bt_set == BtOutcome::NoDevice
    };
    if !net_ok || !bt_ok || net.networks_supported || bt.devices_supported {
        return Err(format!("radios {net:?}/{airplane:?} {bt:?}/{bt_set:?}"));
    }
    let drives = latest::<VolumesState>(shell, "volumes state", |_| true)?;
    let eject = call(shell, &Eject { id: "sdz9".into() }, "volumes.eject")?.outcome;
    if eject != EjectOutcome::Unknown {
        return Err(format!("eject {eject:?}"));
    }
    println!(
        "DIENSTE: removable drives mounted={} (eject of an unknown drive: {eject:?})",
        drives.mounted.len()
    );
    println!(
        "DIENSTE: network wired={} wifi={} airplane possible={} ({airplane:?}); bluetooth present={} ({bt_set:?})",
        net.wired.is_some(), net.wifi_present, net.airplane_possible, bt.present
    );
    Ok(())
}

fn restart_after_kill(admin: &Client, shell: &Client) -> Result<(), String> {
    let pid = running(admin)
        .into_iter()
        .find(|(n, _, _)| n == "powerd")
        .map(|(_, _, pid)| pid)
        .ok_or("powerd pid")?;
    let target = rustix::process::Pid::from_raw(pid as i32).ok_or("pid")?;
    rustix::process::kill_process(target, rustix::process::Signal::KILL)
        .map_err(|e| e.to_string())?;
    wait("powerd restarted", || {
        running(admin)
            .into_iter()
            .any(|(n, _, p)| n == "powerd" && p != pid)
            .then_some(())
    })?;
    latest::<PowerActions>(shell, "power actions after restart", |a| a.power_off)?;
    println!("DIENSTE: killed power service restarted by init");
    Ok(())
}

/// Every step; the last one ends the machine through init.
pub fn all(run: &Run, admin: &Client) -> Result<(), String> {
    if run.fake_root {
        let state = run.at("root").join(ultar_hal_linux::power::SUSPEND_STATE);
        std::fs::create_dir_all(state.parent().ok_or("root")?).map_err(|e| e.to_string())?;
        std::fs::write(&state, "freeze mem\n").map_err(|e| e.to_string())?;
        // The image gives the power service write access to this file (root:powerd 0660).
        use std::os::unix::fs::PermissionsExt;
        for dir in state
            .ancestors()
            .skip(1)
            .take_while(|d| d.starts_with(run.at("root")))
        {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o666))
            .map_err(|e| e.to_string())?;
    }
    let (lock, locks) = lock_screen(run)?;
    let shell = run.client("system.shell")?;
    sign_in(run, &lock, &shell)?;
    notifications(run, &shell)?;
    calendar(run, &shell)?;
    devices(run, &shell)?;
    media(run, &lock)?;
    radios(&shell)?;
    restart_after_kill(admin, &shell)?;
    let out = call(&shell, &SignOut {}, "session.sign_out")?;
    if out.outcome != SessionOutcome::Done || locks.load(Ordering::SeqCst) == 0 {
        return Err(format!("sign out {:?}", out.outcome));
    }
    latest::<SessionState>(&shell, "signed out", |s| s.user.is_none())?;
    println!("DIENSTE: signed out behind the lock screen");
    let off = call(&shell, &PowerOff {}, "power.off")?;
    if off.outcome != ActionOutcome::Done {
        return Err(format!("power off {:?}", off.outcome));
    }
    println!("DIENSTE: power off through the service manager");
    Ok(())
}
