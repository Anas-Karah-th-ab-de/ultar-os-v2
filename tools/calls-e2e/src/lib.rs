//! End-to-end test crate of calls between devices (tools area): `tests/calls_e2e.rs` runs two real
//! mesh services (`ultar-mesh`) and, on each device, the real call service (`ultar-call`) with a
//! fake line and a fake sound device. Services may not depend on each other
//! (tools/check-layers.toml), tools may; the library itself is empty.
#![forbid(unsafe_code)]
