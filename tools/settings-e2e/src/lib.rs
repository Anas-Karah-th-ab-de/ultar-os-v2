//! End-to-end test crate of the Settings app (tools area): `examples/settingstest.rs` runs the
//! app (`apps/ultar-settings`) against the real settings and volume services under the
//! service manager and checks every wired setting with the code of the service that follows
//! it (`platform/ultar-displayd`, `platform/ultar-desktop`). Apps may not depend on those
//! (tools/check-layers.toml), tools may; the library itself is empty.
#![forbid(unsafe_code)]
