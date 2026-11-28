//! Texts of the kernel's own messages that are not shell words (the message module of ultar-check):
//! errors of the ext4 writer and boot lines. The language-dependent shell words stay in sprache.rs.

/// The ext4 writer found no room (the caller maps it to `ENOSPC`).
pub const EXT4_NO_SPACE: &str = "Datenträger voll";
/// A file whose blocks need more extent leaves than the writer builds.
pub const EXT4_FRAGMENTED: &str = "Datei zu zerstückelt";
/// Inline data (EXT4_INLINE_DATA_FL) is read, never written.
pub const EXT4_INLINE: &str = "Inline-Daten nicht unterstützt";
/// An empty name, one over 255 bytes or a symbolic link target that does not fit.
pub const EXT4_BAD_NAME: &str = "ungültiger Name";
/// Boot line: the second low window belongs to processes.
pub const WINDOW_OPEN: &str = "[speicher] Fenster 0x7c000000-0x80000000 für feste Adressen (Wine, Windows-Programme): frei";
/// Boot line: firmware data, the framebuffer or device memory keeps the window closed.
pub const WINDOW_CLOSED: &str = "[speicher] Fenster 0x7c000000-0x80000000: Firmware oder Bildspeicher liegt dort – Programme mit festen Adressen dort (Wine) starten nicht";
