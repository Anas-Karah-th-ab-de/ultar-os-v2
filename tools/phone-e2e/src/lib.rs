//! End-to-end test crate of the phone app (tools area): `tests/e2e.rs` runs the phone app
//! (`apps/ultar-phone`) against the call service (`system/services/ultar-call`) with its test
//! double over a real bus broker. Apps may not depend on services (tools/check-layers.toml),
//! tools may; the library itself is empty.
#![forbid(unsafe_code)]
