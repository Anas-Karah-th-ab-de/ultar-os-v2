        # Auftrag der Treiber-Werkstatt: Treiber „gen-xhci“ für den Ultar-Kern

        Schreibe `kernel/ultar-kern/src/drivers/gen_xhci.rs` vollständig aus (eigener Rust-Code, `no_std` + `alloc`).

        ## Gerät (vom Kern erkannt)
        ```json
        {
  "pci": "00:04.0",
  "hersteller": "1b36",
  "geraet": "000d",
  "subsystem": "1af4:1100",
  "klasse": "0c0330",
  "rev": 1,
  "art": "USB 3 (xHCI)",
  "bauplan": "xhci",
  "norm": "eXtensible Host Controller Interface 1.2 (Intel)",
  "bars": [
    {
      "bar": 0,
      "adresse": "0xc000004000",
      "groesse": 16384,
      "io": false
    }
  ],
  "faehigkeiten": [
    17,
    16
  ],
  "fehler": null
}
        ```

        ## Grundlage
        - Norm/Datenblatt: eXtensible Host Controller Interface 1.2, Intel – Kapitel 4 (Betrieb), 5 (Register), 6 (Datenstrukturen)
        - **Keinen Linux-Treibercode übernehmen** (GPL). Linux nur als Hinweis auf Eigenheiten lesen, Code neu schreiben.

        ## Schnittstelle des Kerns (siehe `src/driver.rs`)
        - `Driver::matches/probe` – `probe` richtet das Gerät ein und trägt es in seine Klasse ein:
          Datenträger → `BlockDev` in `driver::BLOCK`, Netzwerk → `NetDev` in `driver::NET`.
        - DMA-Speicher: `crate::mem::alloc_pages(n)` (genullt, physisch = virtuell). MMIO über BAR-Adressen mit
          `read_volatile/write_volatile`. Zeit: `crate::arch::uptime_ms()`, `pause()`. Keine Interrupts (Abfragebetrieb).
        - Jede Warteschleife mit Zeitgrenze; Fehler als `Err(String)` zurückgeben, nie anhalten oder `panic!`.
        - Erfolgsmeldung mit `crate::kprintln!` so, dass dieses Muster passt: `\[xhci\] .* Port`

        ## Prüfung (automatisch)
        `tools/treiber-werkstatt/werkstatt.py pruefen xhci` baut den Kern und startet QEMU mit
        `-device qemu-xhci,id=x -device usb-kbd,bus=x.0`. Bestanden, wenn `-> gen-xhci (werkstatt) ok` und das Muster oben im Protokoll stehen.
