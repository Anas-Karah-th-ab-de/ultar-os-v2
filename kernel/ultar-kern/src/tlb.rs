//! TLB-Abgleich (shootdown) zwischen den Prozessorkernen.
//!
//! Hebt ein Kern eine Benutzer-Abbildung auf oder verengt ihre Rechte, hält jeder andere Kern, der
//! diesen Adressraum gerade geladen hat, unter Umständen noch eine veraltete Übersetzung im TLB.
//! Eine veraltete Übersetzung auf eine Seite, die bereits freigegeben und neu vergeben wurde, lässt
//! einen Faden die neue Seite lesen oder schreiben – genau das Sicherheitsloch, das dieser Baustein
//! schließt. Der Aufrufer räumt daher zuerst die Seitentabellen-Einträge (und `invlpg` auf dem
//! eigenen Kern), sammelt die freizugebenden Seiten ein und ruft dann – *ohne* die Prozesstabelle zu
//! halten – [`shoot`]. Erst wenn jeder angesprochene Kern quittiert hat, dürfen die Seiten frei
//! werden; andernfalls könnte ein Kern noch durch seinen veralteten Eintrag hineinschreiben.
//!
//! Weder PCID noch globale Seiten werden benutzt: ein `mov cr3` leert daher jede Übersetzung. Ein
//! Kern hält veraltete Einträge also genau so lange, wie er den Adressraum geladen hat – [`USER`]
//! merkt sich je Kern den geladenen (oder gleich geladenen) Benutzer-Adressraum (0 = keiner). Ein
//! Abgleich richtet sich nur an die Kerne, deren [`USER`] der Adressraum ist (Einzelkern-Schnellpfad:
//! niemand hält ihn, kein IPI).
//!
//! Nur der sendende Kern nimmt die Sperre [`DOOR`]; der IPI-Behandler liest die Anfrage sperrfrei.
//! Während der Sender auf die Quittungen wartet, bleiben Unterbrechungen an: er bedient selbst
//! eingehende Abgleiche (zwei Kerne, die gleichzeitig abgleichen, verkeilen sich nicht).

use crate::arch::MAXCPU;
use core::sync::atomic::{AtomicU64, Ordering};

/// Unterbrechungs-Vektor des Abgleich-IPI (fester, freier Vektor, siehe trap.rs).
pub const VEC_TLB: u64 = 0x43;

/// Bis zu so vielen Seiten wird der Bereich Seite für Seite (`invlpg`) ungültig gemacht; darüber
/// lädt der Zielkern CR3 neu (ein `mov cr3` leert den ganzen TLB).
const FLUSH_THRESHOLD: u64 = 32 * 4096;

/// pml4 des Benutzer-Adressraums, den jeder Kern gerade (oder gleich) geladen hat; 0 = keiner.
static USER: [AtomicU64; MAXCPU] = [const { AtomicU64::new(0) }; MAXCPU];

/// Die eine laufende Abgleich-Anfrage. [`DOOR`] reiht Sender hintereinander; der IPI-Behandler liest
/// die Felder sperrfrei (der Sender schreibt sie fertig und macht sie über [`GEN`] sichtbar).
struct Req {
    space: AtomicU64,
    start: AtomicU64,
    /// `end == 0`: CR3 neu laden (große oder ganze Bereiche).
    end: AtomicU64,
}
static REQ: Req = Req { space: AtomicU64::new(0), start: AtomicU64::new(0), end: AtomicU64::new(0) };
static GEN: AtomicU64 = AtomicU64::new(0);
/// Generation, die der jeweilige Kern zuletzt quittiert hat.
static ACK: [AtomicU64; MAXCPU] = [const { AtomicU64::new(0) }; MAXCPU];
static DOOR: spin::Mutex<()> = spin::Mutex::new(());

/// Seiten, die [`paging::AddrSpace::detach`] aus der Seitentabelle geräumt hat und die erst nach dem
/// Abgleich freigegeben werden dürfen. Außerhalb der Prozesstabelle auflösen (siehe [`shoot`]).
pub struct Teardown {
    pub pml4: u64,
    pub start: u64,
    pub end: u64,
    pub freed: alloc::vec::Vec<u64>,
}

impl Teardown {
    /// Erst die anderen Kerne abgleichen, dann die Seiten frei geben.
    pub fn resolve(self) {
        shoot(self.pml4, self.start, self.end);
        for pa in self.freed {
            crate::mem::free_page(pa);
        }
    }
}

/// Dieser Kern lädt gleich den Benutzer-Adressraum `pml4` (vor dem CR3-Wechsel aufgerufen, damit ein
/// gleichzeitiger Abgleich keinen Kern übersieht, der gleich dessen Übersetzungen hält).
pub fn enter(pml4: u64) {
    USER[crate::percpu::id()].store(pml4, Ordering::SeqCst);
}

/// Dieser Kern hat seinen Benutzer-Adressraum verlassen (zurück zu den Kern-Abbildungen).
pub fn leave() {
    USER[crate::percpu::id()].store(0, Ordering::SeqCst);
}

/// Den Bereich `[start, end)` des Adressraums `pml4` auf jedem anderen Kern, der ihn gerade lädt,
/// ungültig machen (der Aufrufer hat seine eigenen Übersetzungen bereits geleert). Ohne Sperren und
/// mit Unterbrechungen an aufrufen; erst nach der Rückkehr sind die im zugehörigen [`Teardown`]
/// gesammelten Seiten zur Freigabe freigegeben.
pub fn shoot(pml4: u64, start: u64, end: u64) {
    let me = crate::percpu::id();
    let online = crate::apic::ONLINE.load(Ordering::SeqCst) as usize;
    // Einzelkern-Schnellpfad: kein anderer Kern hält den Adressraum – kein IPI.
    let mut targets = 0u64;
    for cpu in 0..online.min(MAXCPU) {
        if cpu != me && USER[cpu].load(Ordering::SeqCst) == pml4 {
            targets |= 1 << cpu;
        }
    }
    if targets == 0 {
        return;
    }

    let _door = DOOR.lock();
    REQ.space.store(pml4, Ordering::Relaxed);
    REQ.start.store(start, Ordering::Relaxed);
    REQ.end.store(if end - start > FLUSH_THRESHOLD { 0 } else { end }, Ordering::Relaxed);
    let gen = GEN.load(Ordering::Relaxed) + 1;
    // Anfrage sichtbar machen, dann alle anderen Kerne ansprechen.
    GEN.store(gen, Ordering::Release);
    crate::apic::ipi_all_others(VEC_TLB as u32);

    // auf jede Quittung warten (der IPI geht an alle, daher quittieren alle angebundenen Kerne);
    // Unterbrechungen bleiben an, dieser Kern bedient eingehende Abgleiche während des Wartens.
    let t0 = crate::arch::uptime_ms();
    loop {
        let mut done = true;
        for cpu in 0..online.min(MAXCPU) {
            if cpu != me && ACK[cpu].load(Ordering::Acquire) != gen {
                done = false;
                break;
            }
        }
        if done {
            return;
        }
        if crate::arch::uptime_ms() - t0 > 5000 {
            // Kommt normal nie vor; ein Hängen wäre schlimmer. Eine Warnung, dann weiter.
            crate::kprintln!("[tlb] kein Abgleich von Kern {me} für Adressraum {pml4:#x}");
            return;
        }
        crate::arch::pause();
    }
}

/// IPI-Behandler von [`VEC_TLB`]: die laufende Anfrage auf diesem Kern ausführen und quittieren.
pub fn handle() {
    let gen = GEN.load(Ordering::Acquire);
    if gen == 0 {
        return; // noch nie eine Anfrage: nichts zu quittieren
    }
    if REQ.end.load(Ordering::Relaxed) == 0 {
        // SAFETY: CR3 lesen und denselben Wert zurückschreiben leert den TLB; in jedem Kontext
        // (Kern oder Benutzer) unbedenklich und berührt keinen Speicher.
        unsafe { crate::paging::flush_all() };
    } else {
        let mut a = REQ.start.load(Ordering::Relaxed);
        let end = REQ.end.load(Ordering::Relaxed);
        while a < end {
            // SAFETY: `a` ist eine kanonische Benutzeradresse (vom Systemaufruf begrenzt); invlpg
            // berührt keinen Speicher.
            unsafe { core::arch::asm!("invlpg [{}]", in(reg) a) };
            a += 4096;
        }
    }
    ACK[crate::percpu::id()].store(gen, Ordering::Release);
}