        # Auftrag der Treiber-Werkstatt: Treiber „gen-intel-eth“ für den Ultar-Kern

        Schreibe `kernel/ultar-kern/src/drivers/gen_intel_eth.rs` vollständig aus (eigener Rust-Code, `no_std` + `alloc`).

        ## Gerät (vom Kern erkannt)
        ```json
        {
  "pci": "00:03.0",
  "hersteller": "8086",
  "geraet": "10d3",
  "subsystem": "8086:0000",
  "klasse": "020000",
  "rev": 0,
  "art": "Ethernet",
  "bauplan": "intel-eth",
  "norm": "Intel-Ethernet-Datenblatt (e1000/e1000e/igc, öffentlich)",
  "bars": [
    {
      "bar": 0,
      "adresse": "0x810a0000",
      "groesse": 131072,
      "io": false
    },
    {
      "bar": 1,
      "adresse": "0x81080000",
      "groesse": 131072,
      "io": false
    },
    {
      "bar": 2,
      "adresse": "0x60e0",
      "groesse": 32,
      "io": true
    },
    {
      "bar": 3,
      "adresse": "0x810c0000",
      "groesse": 16384,
      "io": false
    }
  ],
  "faehigkeiten": [
    1,
    5,
    16,
    17
  ],
  "fehler": null
}
        ```

        ## Grundlage
        - Norm/Datenblatt: Intel 82574 / I219 Datenblatt (öffentlich): Register CTRL, STATUS, RCTL, TCTL, RDBA/TDBA, Deskriptorringe
        - **Keinen Linux-Treibercode übernehmen** (GPL). Linux nur als Hinweis auf Eigenheiten lesen, Code neu schreiben.

        ## Schnittstelle des Kerns (siehe `src/driver.rs`)
        - `Driver::matches/probe` – `probe` richtet das Gerät ein und trägt es in seine Klasse ein:
          Datenträger → `BlockDev` in `driver::BLOCK`, Netzwerk → `NetDev` in `driver::NET`.
        - DMA-Speicher: `crate::mem::alloc_pages(n)` (genullt, physisch = virtuell). MMIO über BAR-Adressen mit
          `read_volatile/write_volatile`. Zeit: `crate::arch::uptime_ms()`, `pause()`. Keine Interrupts (Abfragebetrieb).
        - Jede Warteschleife mit Zeitgrenze; Fehler als `Err(String)` zurückgeben, nie anhalten oder `panic!`.
        - Erfolgsmeldung mit `crate::kprintln!` so, dass dieses Muster passt: `\[intel-eth\] .* MAC`

        ## Prüfung (automatisch)
        `tools/treiber-werkstatt/werkstatt.py pruefen intel-eth` baut den Kern und startet QEMU mit
        `-netdev user,id=e -device e1000e,netdev=e`. Bestanden, wenn `-> gen-intel-eth (werkstatt) ok` und das Muster oben im Protokoll stehen.
