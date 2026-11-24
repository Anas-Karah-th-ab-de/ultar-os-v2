//! The calls of `display.*` (except `detect`, which waits for the
//! compositor and is answered by the loop).

use crate::changes::settings_for;
use crate::core::{Core, Ports, KEEP_SECONDS};
use crate::ports::PortError;
use ultar_bus::messages::display_service::{
    code, Apply, ApplyReply, Identify, IdentifyReply, Keep, ResolveReply, IDENTIFY_SECONDS,
};
use ultar_surface::proto::Request;

fn settings_code(e: PortError) -> u32 {
    match e {
        PortError::Unavailable => code::SETTINGS_UNAVAILABLE,
        PortError::Refused(_) => code::INVALID,
    }
}

/// `display.apply`: the settings of the change, each provisional in
/// `settingsd`. Settings that already have the value are left alone; when
/// one is refused, the ones set before it are taken back (all or nothing).
pub fn apply(core: &mut Core, ports: &Ports<'_>, call: &Apply) -> Result<ApplyReply, u32> {
    let wanted =
        settings_for(&call.change, core.connected(), core.values()).map_err(|e| e.code())?;
    let mut done: Vec<String> = Vec::new();
    for (key, value) in wanted {
        if core.raw(&key) == Some(&value) {
            continue;
        }
        if let Err(e) = ports.settings.set(&key, value) {
            for key in &done {
                let _ = ports.settings.revert(key);
            }
            return Err(settings_code(e));
        }
        done.push(key);
    }
    let seconds = if done.is_empty() { 0 } else { KEEP_SECONDS };
    Ok(ApplyReply {
        pending: done,
        seconds,
    })
}

/// `display.keep`: keeps every waiting display change, or (`more`) gives
/// the full time once more.
pub fn keep(core: &mut Core, ports: &Ports<'_>, call: Keep) -> Result<ResolveReply, u32> {
    let keys: Vec<String> = core.pending().keys().cloned().collect();
    if keys.is_empty() {
        return Err(code::NOT_PENDING);
    }
    let mut resolved = 0;
    for key in &keys {
        let outcome = if call.more {
            match core.raw(key).cloned() {
                Some(value) => ports.settings.set(key, value).map(|()| 1),
                None => Ok(0),
            }
        } else {
            ports.settings.confirm(key)
        };
        resolved += outcome.map_err(settings_code)?;
    }
    if call.more {
        core.rearm();
    }
    Ok(ResolveReply { resolved })
}

/// `display.revert`: takes every waiting display change back now.
pub fn revert(core: &mut Core, ports: &Ports<'_>) -> Result<ResolveReply, u32> {
    let keys: Vec<String> = core.pending().keys().cloned().collect();
    if keys.is_empty() {
        return Err(code::NOT_PENDING);
    }
    let mut resolved = 0;
    for key in &keys {
        resolved += ports.settings.revert(key).map_err(settings_code)?;
        core.on_resolved(key);
    }
    Ok(ResolveReply { resolved })
}

/// `display.identify`: the numbers on the displays for 5 s.
pub fn identify(core: &Core, ports: &Ports<'_>, call: &Identify) -> Result<IdentifyReply, u32> {
    let numbers = core.numbers(&call.order);
    let shown = numbers.len() as u32;
    let sent = ports.compositor.send(Request::IdentifyOutputs {
        numbers,
        ms: IDENTIFY_SECONDS * 1000,
    });
    if !sent {
        return Err(code::NO_COMPOSITOR);
    }
    Ok(IdentifyReply { shown })
}
