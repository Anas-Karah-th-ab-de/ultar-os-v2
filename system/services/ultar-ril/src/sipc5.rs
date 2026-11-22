//! SIPC5-Link-Layer – Rahmenformat des Samsung-Shannon-Modems (modem_v1).
//!
//! WICHTIG: Beim Zugriff über die Zeichengeräte /dev/umts_ipc0 usw. baut/entfernt der
//! KERNEL den SIPC5-Header selbst (je nach iod,id des Knotens). Der Userspace schreibt/liest
//! daher die ROHEN IPC-Nutzdaten. Dieses Modul ist für (a) das Verständnis/die Doku,
//! (b) das Interpretieren roher Mitschnitte (riltest) und (c) Unit-Tests. Es wird nur dann
//! zum tatsächlichen Rahmen gebraucht, wenn man je einen rohen Link ohne Treiber-Framing nutzt.
//!
//! Headeraufbau (aus a30k drivers/misc/modem_v1/include/sipc5.h):
//!   Byte0 cfg : obere 5 Bit = Startmarke (0b11111000 = 0xF8), untere 3 Bit = Flags
//!              0x04 PADDING_EXIST, 0x02 EXT_FIELD_EXIST, 0x01 CTL_FIELD_EXIST
//!   Byte1 ch  : Kanal-ID
//!   Byte2..3  : len (u16, little-endian) = Gesamtlänge Frame (Header+Payload) OHNE Padding
//!   danach optional: CTL (1 Byte, bei Multiframe) ODER ext_len (2 Byte, bei EXT ohne Multi)
//!
//! Minimaler Header = 4 Byte. Padding richtet die Gesamtlänge auf 4 Byte (BITS_PER_LONG/8 = 8? siehe Treiber:
//! sipc5_calc_padding_size nutzt BITS_PER_LONG/8; auf arm64 = 8). Wir richten auf 4 aus dem aligned-Flag;
//! die genaue Padding-Größe liefert pad_size().

pub const SIPC5_START_MASK: u8 = 0b1111_1000; // 0xF8
pub const SIPC5_PADDING_EXIST: u8 = 0b0000_0100;
pub const SIPC5_EXT_FIELD_EXIST: u8 = 0b0000_0010;
pub const SIPC5_CTL_FIELD_EXIST: u8 = 0b0000_0001;
pub const SIPC5_MULTI_FRAME_CFG: u8 = 0b0000_0011;
pub const SIPC5_EXT_LENGTH_CFG: u8 = 0b0000_0010;

pub const SIPC5_MIN_HEADER_SIZE: usize = 4;
pub const SIPC5_HEADER_SIZE_WITH_CTL: usize = 5;
pub const SIPC5_HEADER_SIZE_WITH_EXT_LEN: usize = 6;

/// true, wenn das Byte eine gültige SIPC5-Startmarke trägt.
pub fn start_valid(first: u8) -> bool {
    (first & SIPC5_START_MASK) == SIPC5_START_MASK
}

/// Padding-Größe auf arm64 (Ausrichtung auf 8 Byte), wie sipc5_calc_padding_size.
pub fn pad_size(len: usize) -> usize {
    let residue = len & (8 - 1);
    if residue != 0 {
        8 - residue
    } else {
        0
    }
}

/// Headerlänge aus dem ersten Byte bestimmen.
pub fn hdr_len(first: u8) -> usize {
    if first & SIPC5_EXT_FIELD_EXIST != 0 {
        if (first & 0b11) == SIPC5_MULTI_FRAME_CFG {
            SIPC5_HEADER_SIZE_WITH_CTL
        } else {
            SIPC5_HEADER_SIZE_WITH_EXT_LEN
        }
    } else {
        SIPC5_MIN_HEADER_SIZE
    }
}

/// Einen einfachen SIPC5-Rahmen (ein Frame, kein Multiframe) um die Nutzdaten bauen.
/// `aligned` setzt das Padding-Flag und füllt auf 8 Byte auf (wie der Treiber bei ld->aligned).
/// Rückgabe: vollständiger Rahmen (Header + Payload + ggf. Padding).
pub fn build_frame(channel: u8, payload: &[u8], aligned: bool) -> Vec<u8> {
    let hdr = SIPC5_MIN_HEADER_SIZE;
    let frame_len = hdr + payload.len(); // len-Feld = Header+Payload ohne Padding
    let mut cfg = SIPC5_START_MASK;
    if aligned {
        cfg |= SIPC5_PADDING_EXIST;
    }
    let mut out = Vec::with_capacity(frame_len + 8);
    out.push(cfg);
    out.push(channel);
    out.extend_from_slice(&(frame_len as u16).to_le_bytes());
    out.extend_from_slice(payload);
    if aligned {
        out.extend(std::iter::repeat(0u8).take(pad_size(frame_len)));
    }
    out
}

/// Einen empfangenen Rohpuffer parsen. Liefert (headerlänge, kanal, frame_len ohne padding, payload-slice).
pub fn parse_frame(buf: &[u8]) -> Option<(usize, u8, usize, &[u8])> {
    if buf.len() < SIPC5_MIN_HEADER_SIZE || !start_valid(buf[0]) {
        return None;
    }
    let hl = hdr_len(buf[0]);
    if buf.len() < hl {
        return None;
    }
    let ch = buf[1];
    // len-Feld: bei EXT ohne Multiframe 4 Byte (u32), sonst 2 Byte (u16)
    let frame_len = if (buf[0] & SIPC5_EXT_FIELD_EXIST != 0) && ((buf[0] & 0b11) != SIPC5_MULTI_FRAME_CFG) {
        u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]) as usize
    } else {
        u16::from_le_bytes([buf[2], buf[3]]) as usize
    };
    if frame_len < hl || frame_len > buf.len() {
        // Payload evtl. unvollständig – nur Header+Längen zurückgeben, Payload soweit vorhanden
        let end = buf.len().min(frame_len.max(hl));
        return Some((hl, ch, frame_len, &buf[hl..end]));
    }
    Some((hl, ch, frame_len, &buf[hl..frame_len]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_then_parse_roundtrip() {
        let payload = [0x01, 0x02, 0x03, 0x04, 0x05];
        let frame = build_frame(235, &payload, false);
        // Header: cfg=0xF8, ch=235, len=4+5=9
        assert_eq!(frame[0], 0xF8);
        assert_eq!(frame[1], 235);
        assert_eq!(u16::from_le_bytes([frame[2], frame[3]]), 9);
        let (hl, ch, flen, pl) = parse_frame(&frame).unwrap();
        assert_eq!(hl, 4);
        assert_eq!(ch, 235);
        assert_eq!(flen, 9);
        assert_eq!(pl, &payload);
    }

    #[test]
    fn aligned_adds_padding() {
        let payload = [0xAA; 5]; // frame_len = 9 -> pad auf 16 => 7 Byte padding
        let frame = build_frame(235, &payload, true);
        assert_eq!(frame[0] & SIPC5_PADDING_EXIST, SIPC5_PADDING_EXIST);
        assert_eq!(frame.len(), 9 + pad_size(9));
    }

    #[test]
    fn rejects_bad_start() {
        assert!(parse_frame(&[0x00, 235, 4, 0]).is_none());
    }
}
