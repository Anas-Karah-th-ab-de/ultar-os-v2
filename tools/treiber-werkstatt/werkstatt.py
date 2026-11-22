#!/usr/bin/env python3
"""Ultar Treiber-Werkstatt – Treiber für den Ultar-Kern erzeugen, bauen und prüfen.

Ablauf (jeder Schritt einzeln aufrufbar):
  erfassen <protokoll>     Baupläne aus dem Kernprotokoll (Zeilen „WERKSTATT {...}“) nach werkstatt/plaene/
  erzeugen <bauplan|art>   Treiber-Gerüst in src/drivers/gen_<art>.rs + Eintrag in drivers/mod.rs
                           + Auftrag werkstatt/auftraege/<art>.md (Norm, Register, Schnittstelle, Prüfung)
  ki <art> [--claude|--lokal URL]
                           Auftrag an ein Sprachmodell: Claude Code (eigenes Abo) oder das lokale Modell
                           (OpenAI-kompatibel, z. B. ultar-llm auf 127.0.0.1:8080) – schreibt den Treiber aus
  pruefen <art>            Kern bauen, in QEMU mit passendem Gerät starten, Erfolg im Protokoll prüfen
  alles <protokoll>        erfassen → erzeugen → ki → pruefen für jeden Bauplan

Geprüft wird immer in QEMU, nie zuerst auf echter Hardware: ein erzeugter Treiber gilt erst als gut, wenn
„[treiber] … -> gen-<art> (werkstatt) ok“ und die Prüfzeilen der Art im Protokoll stehen.
"""
import json, os, re, subprocess, sys, pathlib, textwrap, urllib.request

KERN = pathlib.Path(__file__).resolve().parents[2] / "kernel" / "ultar-kern"
W = KERN / "werkstatt"
MOD = KERN / "src" / "drivers" / "mod.rs"

# Art → (Norm-Dokument, QEMU-Gerät zum Prüfen, erwartete Prüfzeile im Protokoll)
ARTEN = {
    "ahci": ("Serial ATA AHCI 1.3.1, Intel – Abschnitte 3 (Register), 4 (Systemspeicher-Strukturen), 5 (Datenübertragung), 10.1 (Initialisierung)",
             "-drive file={disk},if=none,id=sd,format=raw -device ahci,id=ahci0 -device ide-hd,drive=sd,bus=ahci0.0",
             r"\[ahci\] sata\d+: .* MB"),
    "xhci": ("eXtensible Host Controller Interface 1.2, Intel – Kapitel 4 (Betrieb), 5 (Register), 6 (Datenstrukturen)",
             "-device qemu-xhci,id=x -device usb-kbd,bus=x.0", r"\[xhci\] .* Port"),
    "intel-eth": ("Intel 82574 / I219 Datenblatt (öffentlich): Register CTRL, STATUS, RCTL, TCTL, RDBA/TDBA, Deskriptorringe",
                  "-netdev user,id=e -device e1000e,netdev=e", r"\[netz\] DHCP: IP"),
    "hda": ("High Definition Audio 1.0a, Intel – CORB/RIRB, Codec-Verben, Stream-Deskriptoren",
            "-device intel-hda -device hda-output", r"\[hda\] Codec"),
    "vga-fb": ("Bochs VBE-DISPI (Ports 0x1CE/0x1CF bzw. MMIO BAR2+0x500) – Auflösung umschalten, Bildspeicher in BAR0",
               "-vga std", r"\[vga-fb\] \d+x\d+"),
    "smbus": ("Intel ICH9/PCH SMBus-Controller (Datenblatt, Abschnitt SMBus I/O-Register)", "", r"\[smbus\] "),
}

def sh(cmd, **kw):
    print("$", cmd if isinstance(cmd, str) else " ".join(cmd))
    return subprocess.run(cmd, shell=isinstance(cmd, str), **kw)

def erfassen(log):
    (W / "plaene").mkdir(parents=True, exist_ok=True)
    n = 0
    for line in pathlib.Path(log).read_text(errors="replace").splitlines():
        m = re.search(r"WERKSTATT (\{.*\})\s*$", line)
        if not m:
            continue
        p = json.loads(m.group(1))
        name = f"{p['bauplan']}-{p['hersteller']}-{p['geraet']}.json"
        (W / "plaene" / name).write_text(json.dumps(p, indent=2, ensure_ascii=False))
        print(f"Bauplan: {name}  ({p['art']}, Norm: {p['norm']})")
        n += 1
    print(f"{n} Bauplan/Baupläne erfasst")

def plan_fuer(art):
    for f in sorted((W / "plaene").glob(f"{art}-*.json")):
        return json.loads(f.read_text())
    return None

TEMPLATE = '''//! Ultar-Kern – Treiber „gen-{art}“ (Treiber-Werkstatt).
//! Gerät: {art_name} {vendor}:{device} (Klasse {klasse}) · Norm: {norm}
//! Auftrag: werkstatt/auftraege/{art}.md · Status: GERÜST – noch nicht ausgeschrieben.

use crate::driver::{{Driver, Result}};
use crate::pci::PciDev;

pub struct Drv;
pub static DRV: Drv = Drv;

impl Driver for Drv {{
    fn name(&self) -> &'static str {{
        "gen-{art}"
    }}
    fn origin(&self) -> &'static str {{
        "werkstatt"
    }}
    fn matches(&self, d: &PciDev) -> bool {{
        d.class == 0x{c} && d.subclass == 0x{s} && d.prog_if == 0x{p}
    }}
    fn probe(&self, _d: &PciDev) -> Result<()> {{
        Err("gen-{art}: Gerüst – Treiber noch nicht ausgeschrieben (werkstatt ki {art})".into())
    }}
}}
'''

def erzeugen(arg):
    p = json.loads(pathlib.Path(arg).read_text()) if arg.endswith(".json") else plan_fuer(arg)
    if not p:
        sys.exit(f"kein Bauplan für {arg} (erst: erfassen <protokoll>)")
    art = p["bauplan"].replace("-", "_")
    src = KERN / "src" / "drivers" / f"gen_{art}.rs"
    if not src.exists():
        k = p["klasse"]
        src.write_text(TEMPLATE.format(art=p["bauplan"], art_name=p["art"], vendor=p["hersteller"], device=p["geraet"], klasse=k,
                                       norm=p["norm"], c=k[0:2], s=k[2:4], p=k[4:6]))
        print(f"Gerüst: {src.relative_to(KERN)}")
    m = MOD.read_text()
    if f"pub mod gen_{art};" not in m:
        m = m.replace("// WERKSTATT-MODULE-ENDE", f"pub mod gen_{art};\n// WERKSTATT-MODULE-ENDE")
        m = m.replace("    // WERKSTATT-TREIBER-ENDE", f"    &gen_{art}::DRV,\n    // WERKSTATT-TREIBER-ENDE")
        MOD.write_text(m)
        print("in drivers/mod.rs eingetragen")
    (W / "auftraege").mkdir(parents=True, exist_ok=True)
    norm, qemu, probe = ARTEN.get(p["bauplan"], (p["norm"], "", r"ok"))
    auftrag = W / "auftraege" / f"{p['bauplan']}.md"
    auftrag.write_text(textwrap.dedent(f"""\
        # Auftrag der Treiber-Werkstatt: Treiber „gen-{p['bauplan']}“ für den Ultar-Kern

        Schreibe `kernel/ultar-kern/src/drivers/gen_{art}.rs` vollständig aus (eigener Rust-Code, `no_std` + `alloc`).

        ## Gerät (vom Kern erkannt)
        ```json
        {json.dumps(p, indent=2, ensure_ascii=False)}
        ```

        ## Grundlage
        - Norm/Datenblatt: {norm}
        - **Keinen Linux-Treibercode übernehmen** (GPL). Linux nur als Hinweis auf Eigenheiten lesen, Code neu schreiben.

        ## Schnittstelle des Kerns (siehe `src/driver.rs`)
        - `Driver::matches/probe` – `probe` richtet das Gerät ein und trägt es in seine Klasse ein:
          Datenträger → `BlockDev` in `driver::BLOCK`, Netzwerk → `NetDev` in `driver::NET`.
        - DMA-Speicher: `crate::mem::alloc_pages(n)` (genullt, physisch = virtuell). MMIO über BAR-Adressen mit
          `read_volatile/write_volatile`. Zeit: `crate::arch::uptime_ms()`, `pause()`. Keine Interrupts (Abfragebetrieb).
        - Jede Warteschleife mit Zeitgrenze; Fehler als `Err(String)` zurückgeben, nie anhalten oder `panic!`.
        - Erfolgsmeldung mit `crate::kprintln!` so, dass dieses Muster passt: `{probe}`

        ## Prüfung (automatisch)
        `tools/treiber-werkstatt/werkstatt.py pruefen {p['bauplan']}` baut den Kern und startet QEMU mit
        `{qemu}`. Bestanden, wenn `-> gen-{p['bauplan']} (werkstatt) ok` und das Muster oben im Protokoll stehen.
        """))
    print(f"Auftrag: {auftrag.relative_to(KERN)}")

def ki(art, mode="--claude", url="http://127.0.0.1:8080/v1/chat/completions"):
    auftrag = (W / "auftraege" / f"{art}.md").read_text()
    datei = KERN / "src" / "drivers" / f"gen_{art.replace('-', '_')}.rs"
    if mode == "--claude":
        # Claude Code im Kopf-losen Modus mit dem eigenen Abo (vorher einmal `claude` → /login)
        sh(["claude", "-p", auftrag, "--allowedTools", "Read,Edit,Write,Bash(cargo build*)"], cwd=KERN.parents[1])
    else:
        body = {"model": "local", "messages": [
            {"role": "system", "content": "Du schreibst Rust-Treiber für den Ultar-Kern. Antworte nur mit der vollständigen Rust-Datei."},
            {"role": "user", "content": auftrag + "\n\nAktuelle Datei:\n```rust\n" + datei.read_text() + "\n```"}]}
        req = urllib.request.Request(url, json.dumps(body).encode(), {"Content-Type": "application/json"})
        out = json.load(urllib.request.urlopen(req, timeout=1800))["choices"][0]["message"]["content"]
        m = re.search(r"```rust\n(.*?)```", out, re.S)
        datei.write_text(m.group(1) if m else out)
        print(f"lokales Modell hat {datei.name} geschrieben")

def bauen():
    env = dict(os.environ, RUSTC_BOOTSTRAP="1")
    if pathlib.Path("/usr/lib/rust-1.97/bin").exists():
        env["PATH"] = "/usr/lib/rust-1.97/bin:" + env["PATH"]
    r = sh(["cargo", "build", "--release"], cwd=KERN, env=env)
    if r.returncode:
        sys.exit("Bau fehlgeschlagen")
    return KERN / "target" / "x86_64-unknown-uefi" / "release" / "ultar-kern.efi"

def pruefen(art, extra=None):
    efi = bauen()
    _, qemu, probe = ARTEN.get(art, ("", "", r"ok"))
    tmp = pathlib.Path("/tmp/ultar-werkstatt"); tmp.mkdir(exist_ok=True)
    esp, disk = tmp / "esp.img", tmp / "disk.img"
    if not esp.exists():
        sh(f"truncate -s 64M {esp} && mkfs.vfat -F 32 {esp} >/dev/null && mmd -i {esp} ::/EFI ::/EFI/BOOT")
    sh(f"mcopy -o -i {esp} {efi} ::/EFI/BOOT/BOOTX64.EFI")
    if not disk.exists():
        sh(f"truncate -s 256M {disk} && sgdisk -o -n 1:2048:+64M -c 1:ultar-data {disk} >/dev/null")
    code = next(p for p in ["/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_CODE.fd"] if os.path.exists(p))
    vars_ = tmp / "vars.fd"
    sh(f"cp {code.replace('CODE', 'VARS')} {vars_}")
    cmd = (f"qemu-system-x86_64 -machine q35 -m 512 -cpu max -drive if=pflash,format=raw,readonly=on,file={code} "
           f"-drive if=pflash,format=raw,file={vars_} -drive file={esp},if=none,id=boot,format=raw "
           f"-device virtio-blk-pci,drive=boot,bootindex=0 {qemu.format(disk=disk)} {extra or ''} "
           f"-serial file:{tmp}/log.txt -display none -monitor none")
    (tmp / "log.txt").unlink(missing_ok=True)
    sh(f"timeout {os.environ.get('WERKSTATT_ZEIT', '90')} {cmd}")
    log = (tmp / "log.txt").read_text(errors="replace")
    ok_bind = re.search(rf"-> gen-{re.escape(art)} \(werkstatt\) ok", log)
    ok_probe = re.search(probe, log)
    for l in log.splitlines():
        if "[treiber]" in l or f"[{art}]" in l or "AUSNAHME" in l or "PANIK" in l:
            print("  " + l)
    if ok_bind and ok_probe:
        print(f"BESTANDEN: gen-{art}")
        return True
    print(f"NICHT BESTANDEN: gen-{art} (Protokoll {tmp}/log.txt)")
    return False

if __name__ == "__main__":
    a = sys.argv[1:]
    if not a:
        print(__doc__); sys.exit(0)
    {"erfassen": lambda: erfassen(a[1]), "erzeugen": lambda: erzeugen(a[1]),
     "ki": lambda: ki(a[1], *(a[2:] or ["--claude"])),
     "pruefen": lambda: sys.exit(0 if pruefen(a[1]) else 1),
     "alles": lambda: [ (erzeugen(p.stem.split("-")[0]), ki(p.stem.split("-")[0]), pruefen(p.stem.split("-")[0]))
                        for p in (erfassen(a[1]) or sorted((W / "plaene").glob("*.json"))) ]}[a[0]]()
