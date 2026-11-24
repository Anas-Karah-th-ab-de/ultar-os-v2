//! `settings.set_accessibility` and `settings.accessibility`: the sign-in screen's narrow
//! right (`login.access`). Only keys under `accessibility.` are read or written; any other
//! key is refused with `NotPermitted` before the schema is asked, so a caller with this right
//! learns nothing about other settings. Types, ranges and risk classes are the schema's, as
//! for `settings.set`.

use crate::access::Caller;
use crate::core::Core;
use crate::error::SettingsError;
use ultar_bus::messages::settings::{
    is_accessibility_key, AccessibilityReply, SetAccessibility, SetReply, ACCESSIBILITY_PREFIX,
    MAX_ACCESSIBILITY,
};

/// Changes one accessibility setting.
pub(super) fn set(
    core: &mut Core,
    caller: &Caller,
    call: SetAccessibility,
) -> Result<SetReply, SettingsError> {
    if !is_accessibility_key(&call.key) {
        return Err(SettingsError::NotPermitted);
    }
    let changed = core.set(caller, &call.key, call.value.into())?;
    Ok(SetReply { changed })
}

/// The accessibility settings in schema order (those the device has).
pub(super) fn values(core: &Core) -> AccessibilityReply {
    let keys: Vec<&'static str> = core
        .registry()
        .defs()
        .iter()
        .map(|def| def.key)
        .filter(|key| key.starts_with(ACCESSIBILITY_PREFIX) && is_accessibility_key(key))
        .take(MAX_ACCESSIBILITY)
        .collect();
    let values = keys
        .into_iter()
        .filter_map(|key| core.get(key).ok().map(|v| (key.to_owned(), v.into())))
        .collect();
    AccessibilityReply { values }
}
