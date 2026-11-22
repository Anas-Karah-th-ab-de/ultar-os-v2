//! Kernprotokoll im Speicher (Ringpuffer, 64 KB) – für `protokoll` in der Kern-Konsole und die Werkstatt.

const N: usize = 64 * 1024;
static mut BUF: [u8; N] = [0; N];
static mut POS: usize = 0;
static mut WRAPPED: bool = false;

pub fn record(s: &str) {
    unsafe {
        for &b in s.as_bytes() {
            BUF[POS] = b;
            POS += 1;
            if POS == N {
                POS = 0;
                WRAPPED = true;
            }
        }
    }
}

/// Inhalt in zeitlicher Reihenfolge an `f` übergeben
pub fn dump(mut f: impl FnMut(&[u8])) {
    unsafe {
        let buf = &*core::ptr::addr_of!(BUF);
        if WRAPPED {
            f(&buf[POS..]);
        }
        f(&buf[..POS]);
    }
}
