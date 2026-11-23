//! The slot table: 64 bytes of boot control data, and the decision the boot code takes on it.
//!
//! Layout and rules: `docs/SYSTEM-UPDATE.md` §2.5. The code lives in the `no_std` crate
//! `ultar-slots`, which the UEFI loader, the installer and the kernel compile too: the updater
//! and the boot code share one source, not two copies. [`select`] is the start decision the
//! loader takes; the file backend's `simulate_boot` and the tests run the same function.

pub use ultar_slots::table::{
    crc32, plan_store, select, seq_newer, SlotFlags, SlotTable, TableError, TABLE_LEN,
};
