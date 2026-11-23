//! The loader's messages (firmware console, Arabic first). The firmware console may not draw
//! Arabic letters; the English and German lines carry the same text.

/// No slot can be started (both damaged, or the table is unreadable).
pub const NO_SLOT: &str = "\r\nUltar OS: لا يمكن بدء أي نسخة من النظام – صِل وسيط التثبيت.\r\nUltar OS: no system slot can be started - connect the installation medium.\r\nUltar OS: kein Systembereich startbar - bitte das Installationsmedium anschliessen.\r\n";
/// The start partition is not on a disk with the A/B layout.
pub const NO_LAYOUT: &str = "\r\nUltar OS: لم يُعثر على بيانات الإقلاع (ULTAR-BOOTCTL).\r\nUltar OS: boot control data (ULTAR-BOOTCTL) not found.\r\nUltar OS: Startdaten (ULTAR-BOOTCTL) nicht gefunden.\r\n";
/// The rescue program is started.
pub const RESCUE: &str = "\r\nUltar OS: بدء وضع الإنقاذ\r\nUltar OS: starting rescue mode\r\nUltar OS: Rettungsmodus startet\r\n";
