//! Anzeige-Schicht des Ultar-Kerns: eine Schnittstelle, getrennte Treiber je Geräteart.
//!
//! | Geräteart            | Anzeige-Treiber                                                              |
//! |----------------------|------------------------------------------------------------------------------|
//! | PC, Laptop, Server   | `gop` (Bildspeicher der UEFI-Firmware, immer da) · `gen-bochs-vga` (VMs, Modi umschaltbar) · Intel-Xe-Anzeige (Bauplan) |
//! | Handy, Tablet (K8)   | Anzeige-Controller des Herstellers (Exynos DECON, Qualcomm MDSS …) – gleiche Schnittstelle |
//!
//! Konsole, Startseite und `/dev/fb0` lesen den Bildspeicher über `console::framebuffer()`; wechselt die
//! Auflösung, bekommen alle den neuen Bildspeicher.

use crate::efi::Framebuffer;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub trait Anzeige: Send {
    /// kurzer Name, z. B. "gop", "gen-bochs-vga"
    fn name(&self) -> &'static str;
    /// "firmware", "hand" oder "werkstatt"
    fn herkunft(&self) -> &'static str;
    /// mögliche Auflösungen (Breite, Höhe)
    fn modi(&self) -> Vec<(u32, u32)>;
    /// Auflösung einstellen, liefert den neuen Bildspeicher
    fn setzen(&mut self, w: u32, h: u32) -> Result<Framebuffer, String>;
    /// Die Konsole zeichnet jetzt in den neuen Bildspeicher: der alte darf freigegeben werden
    fn umgeschaltet(&mut self) {}
}

static ANZEIGEN: Mutex<Vec<Box<dyn Anzeige>>> = Mutex::new(Vec::new());

/// Bildspeicher der Firmware: feste Auflösung (nach ExitBootServices nicht mehr umschaltbar)
struct Gop {
    fb: Framebuffer,
}

impl Anzeige for Gop {
    fn name(&self) -> &'static str {
        "gop"
    }
    fn herkunft(&self) -> &'static str {
        "firmware"
    }
    fn modi(&self) -> Vec<(u32, u32)> {
        alloc::vec![(self.fb.width, self.fb.height)]
    }
    fn setzen(&mut self, w: u32, h: u32) -> Result<Framebuffer, String> {
        if (w, h) == (self.fb.width, self.fb.height) {
            Ok(self.fb)
        } else {
            Err(format!("Firmware-Bildspeicher hat nur {}x{} (für andere Auflösungen fehlt ein eigener Anzeigetreiber)", self.fb.width, self.fb.height))
        }
    }
}

pub fn init_gop(fb: Option<Framebuffer>) {
    if let Some(fb) = fb {
        ANZEIGEN.lock().push(Box::new(Gop { fb }));
    }
}

/// Eigener Anzeigetreiber meldet sich an; er übernimmt das Umschalten (die Firmware-Anzeige bleibt als Rückfall)
pub fn anmelden(a: Box<dyn Anzeige>) {
    crate::kprintln!(
        "[anzeige] {} ({}): {} Auflösungen",
        a.name(),
        a.herkunft(),
        a.modi().len()
    );
    ANZEIGEN.lock().push(a);
}

/// aktive Anzeige = zuletzt angemeldete (eigener Treiber vor Firmware)
pub fn aktiv() -> Option<&'static str> {
    ANZEIGEN.lock().last().map(|a| a.name())
}

/// Auflösung wechseln (über den aktiven Treiber), Konsole neu aufbauen
pub fn modus(w: u32, h: u32) -> Result<Framebuffer, String> {
    if crate::eingabe::fb_offen() {
        return Err("eine Oberfläche benutzt gerade den Bildschirm".into());
    }
    let fb = {
        let mut g = ANZEIGEN.lock();
        let a = g.last_mut().ok_or("keine Anzeige")?;
        a.setzen(w, h)?
    };
    crate::console::screen_init(Some(fb));
    if let Some(a) = ANZEIGEN.lock().last_mut() {
        a.umgeschaltet();
    }
    Ok(fb)
}

/// Grafikkarten im Rechner und welcher Weg sie betreibt (für /proc/ultar/geraet und die Werkstatt)
fn karten() -> Vec<String> {
    let mut v = Vec::new();
    for b in crate::driver::DEVICES
        .lock()
        .iter()
        .filter(|b| b.dev.class == 0x03)
    {
        let d = &b.dev;
        let weg = match (b.driver, d.vendor) {
            (Some(n), _) => format!("Treiber {n}"),
            (None, 0x8086) => String::from("Firmware-Bildspeicher; eigener Treiber: Bauplan intel-xe-anzeige"),
            (None, 0x10de) => String::from("Firmware-Bildspeicher; NVIDIA: Opt-in-Erkennung vorhanden, kein Ausgabesteuerungstreiber"),
            (None, 0x1002) => String::from("Firmware-Bildspeicher; AMD: Bauplan amd-dcn-anzeige"),
            (None, _) => String::from("Firmware-Bildspeicher"),
        };
        v.push(format!("{:04x}:{:04x} {}", d.vendor, d.device, weg));
    }
    v
}

pub fn text() -> String {
    let mut s = String::new();
    if let Some(f) = crate::console::framebuffer() {
        s += &format!(
            "anzeige={} {}x{}\n",
            aktiv().unwrap_or("-"),
            f.width,
            f.height
        );
    } else {
        s += "anzeige=keine\n";
    }
    for (i, k) in karten().iter().enumerate() {
        s += &format!("grafik{i}={k}\n");
    }
    if let Some(t) = crate::drivers::intel_display::text() {
        s += &format!("intel-display={t}\n");
    }
    if let Some(t) = crate::drivers::intel_display::gt::text() {
        s += &format!("intel-gt={t}\n");
    }
    if let Some(t) = crate::drivers::nvidia_display::text() {
        s += &format!("nvidia-display={t}\n");
    }
    if let Some(t) = crate::drivers::intel_npu::text() {
        s += &format!("intel-npu={t}\n");
    }
    s
}

/// Konsole: `anzeige` (Liste) bzw. `anzeige 1280x720`
pub fn befehl(arg: Option<&str>) {
    match arg {
        None => {
            for a in ANZEIGEN.lock().iter() {
                let m: Vec<String> = a.modi().iter().map(|(w, h)| format!("{w}x{h}")).collect();
                crate::kprintln!("{:<14} {:<9} {}", a.name(), a.herkunft(), m.join(" "));
            }
            for k in karten() {
                crate::kprintln!("Grafikkarte   {k}");
            }
            if let Some(f) = crate::console::framebuffer() {
                crate::kprintln!("aktiv: {} {}x{}", aktiv().unwrap_or("-"), f.width, f.height);
            }
        }
        Some(m) => {
            let mut it = m.split(['x', 'X', '*']);
            let (Some(w), Some(h)) = (
                it.next().and_then(|x| x.parse().ok()),
                it.next().and_then(|x| x.parse().ok()),
            ) else {
                return crate::kprintln!("anzeige <breite>x<höhe>, z. B. anzeige 1920x1080");
            };
            match modus(w, h) {
                Ok(f) => crate::kprintln!(
                    "[anzeige] {}x{} über {}",
                    f.width,
                    f.height,
                    aktiv().unwrap_or("-")
                ),
                Err(e) => crate::kprintln!("[anzeige] {e}"),
            }
        }
    }
}
