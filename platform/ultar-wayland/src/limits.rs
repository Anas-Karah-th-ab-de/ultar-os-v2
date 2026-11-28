//! Hard bounds for everything a client can make the bridge hold. An app that
//! goes beyond one is cut off with a protocol error, never the bridge.

/// Wayland connections of the one app (its processes).
pub const MAX_CLIENTS: usize = 16;
/// Surfaces of one connection.
pub const MAX_SURFACES: usize = 256;
/// Shared-memory pools of one connection.
pub const MAX_POOLS: usize = 128;
/// Buffers of one connection.
pub const MAX_BUFFERS: usize = 1024;
/// Largest pool in bytes (a 8192 × 8192 picture twice).
pub const MAX_POOL_BYTES: u64 = 512 << 20;
/// Largest buffer side in pixels (the compositor's limit).
pub const MAX_SIDE: i32 = ultar_surface::proto::MAX_SIDE as i32;
/// Damage rectangles kept per commit before the whole surface counts.
pub const MAX_DAMAGE: usize = ultar_surface::proto::MAX_DAMAGE;
/// Frame callbacks waiting on one surface.
pub const MAX_CALLBACKS: usize = 64;
/// Subsurfaces below one surface.
pub const MAX_CHILDREN: usize = 32;
/// Positioners of one connection.
pub const MAX_POSITIONERS: usize = 256;
/// MIME types one data source may offer.
pub const MAX_SOURCE_MIMES: usize = 32;
/// Longest MIME type accepted from a client.
pub const MAX_MIME_LEN: usize = 256;
/// Largest clipboard text that travels through the bridge (both ways).
pub const MAX_CLIPBOARD: usize = 16 << 20;
/// Clipboard reads of the app running at the same time.
pub const MAX_READS: usize = 8;
/// Longest title (the compositor's limit, cut at a character boundary).
pub const MAX_TITLE: usize = ultar_surface::proto::MAX_TITLE;
/// Pointer, keyboard, touch and data objects of each kind (all connections).
pub const MAX_INPUT_OBJECTS: usize = 64;
