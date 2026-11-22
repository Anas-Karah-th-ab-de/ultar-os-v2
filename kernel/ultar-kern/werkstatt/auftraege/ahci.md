        # Auftrag der Treiber-Werkstatt: Treiber „gen-ahci“ für den Ultar-Kern

        Schreibe `kernel/ultar-kern/src/drivers/gen_ahci.rs` vollständig aus (eigener Rust-Code, `no_std` + `alloc`).

        ## Gerät (vom Kern erkannt)
        ```json
        {
  "pci": "00:1f.2",
  "hersteller": "8086",
  "geraet": "2922",
  "subsystem": "1af4:1100",
  "klasse": "010601",
  "rev": 2,
  "art": "SATA (AHCI)",
  "bauplan": "ahci",
  "norm": "Serial ATA AHCI 1.3.1 (Intel)",
  "bars": [
    {
      "bar": 4,
      "adresse": "0x60c0",
      "groesse": 32,
      "io": true
    },
    {
      "bar": 5,
      "adresse": "0x81080000",
      "groesse": 4096,
      "io": false
    }
  ],
  "faehigkeiten": [
    5,
    18
  ],
  "fehler": null
}
        ```

        ## Grundlage
        - Norm/Datenblatt: Serial ATA AHCI 1.3.1, Intel – Abschnitte 3 (Register), 4 (Systemspeicher-Strukturen), 5 (Datenübertragung), 10.1 (Initialisierung)
        - **Keinen Linux-Treibercode übernehmen** (GPL). Linux nur als Hinweis auf Eigenheiten lesen, Code neu schreiben.

        ## Schnittstelle des Kerns (siehe `src/driver.rs`)
        - `Driver::matches/probe` – `probe` richtet das Gerät ein und trägt es in seine Klasse ein:
          Datenträger → `BlockDev` in `driver::BLOCK`, Netzwerk → `NetDev` in `driver::NET`.
        - DMA-Speicher: `crate::mem::alloc_pages(n)` (genullt, physisch = virtuell). MMIO über BAR-Adressen mit
          `read_volatile/write_volatile`. Zeit: `crate::arch::uptime_ms()`, `pause()`. Keine Interrupts (Abfragebetrieb).
        - Jede Warteschleife mit Zeitgrenze; Fehler als `Err(String)` zurückgeben, nie anhalten oder `panic!`.
        - Erfolgsmeldung mit `crate::kprintln!` so, dass dieses Muster passt: `\[ahci\] sata\d+: .* MB`

        ## Prüfung (automatisch)
        `tools/treiber-werkstatt/werkstatt.py pruefen ahci` baut den Kern und startet QEMU mit
        `-drive file={disk},if=none,id=sd,format=raw -device ahci,id=ahci0 -device ide-hd,drive=sd,bus=ahci0.0`. Bestanden, wenn `-> gen-ahci (werkstatt) ok` und das Muster oben im Protokoll stehen.
