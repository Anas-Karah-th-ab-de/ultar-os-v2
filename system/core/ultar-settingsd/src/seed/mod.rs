//! The seed schema: every setting of the areas of the desktop
//! specification (§8, §11, §12.1, §12.3) – from displays to emergency.

mod desktop;
mod device;
mod display;
mod services;
mod system;

use crate::schema::{Registry, SchemaError, SettingDef};

/// Schema version of the seed. Raise it together with a migration whenever
/// a stored key is renamed, removed or changes its meaning.
pub const SCHEMA_VERSION: u32 = 1;

/// Seconds a display or network change waits for "keep?" before it is
/// taken back (specification §8: 15 s).
pub const CONFIRM_SECONDS: u32 = 15;

/// Every declaration of the seed in display order.
pub fn definitions() -> Vec<SettingDef> {
    let mut defs = display::definitions();
    defs.extend(desktop::appearance());
    defs.extend(desktop::bar());
    defs.extend(desktop::windows());
    defs.extend(system::notifications());
    defs.extend(system::power());
    defs.extend(system::region());
    defs.extend(system::date_time());
    defs.extend(system::privacy());
    defs.extend(system::account());
    defs.extend(device::mobile());
    defs.extend(device::search());
    defs.extend(device::apps());
    defs.extend(device::pointer());
    defs.extend(device::keyboard());
    defs.extend(device::printers());
    defs.extend(device::colour());
    defs.extend(device::accessibility());
    defs.extend(services::sharing());
    defs.extend(services::users());
    defs.extend(services::storage());
    defs.extend(services::updates());
    defs.extend(services::backup());
    defs.extend(services::mesh());
    defs.extend(services::assistant());
    defs.extend(services::cells());
    defs.extend(services::emergency());
    defs
}

/// The registry of the seed schema.
pub fn registry() -> Result<Registry, SchemaError> {
    Registry::builder()
        .version(SCHEMA_VERSION)
        .settings(definitions())
        .build()
}

#[cfg(test)]
mod tests;
