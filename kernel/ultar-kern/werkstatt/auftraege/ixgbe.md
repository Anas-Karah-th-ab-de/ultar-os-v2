# Auftrag: gen-ixgbe (Intel 10-Gigabit-Ethernet 82599 / X540 / X550 / E610)

Gerät: server3 (Beelink GTi15 Ultra) – Intel E610-XT2, 8086:57b0, Subsystem 8086:0002, BAR0 16 MB (64 Bit, oberhalb 4 GB).

Norm: Intel-Datenblätter 82599 / X550 / E610 (öffentlich). Für die E610-Firmwareschnittstelle (ACI) Konstanten nach
FreeBSD ix(4) (`ixgbe_type_e610.h`, BSD-3). Kein Linux-Code.

Umfang:
- Rücksetzen (GIO_MASTER_DISABLE → CTRL.RST), NVM fertig (EEC.ARD), DRV_LOAD
- MAC aus RAL0/RAH0
- Empfang: Legacy-Deskriptoren (SRRCTL.DESCTYPE 0, 2 KB, DROP_EN), RXDCTL.ENABLE, RXCTRL.RXEN, CRC entfernen (HLREG0 + RDRXCTL)
- Senden: DMATXCTL.TE, TXDCTL.ENABLE, Legacy-Deskriptor EOP|IFCS|RS
- E610: ACI über PF_HIDA/PF_HIBA/PF_HICR – GET_VER (0x0001), RESTART_AN (0x0605, LINK_RESTART|LINK_ENABLE), GET_LINK_STATUS (0x0607)
- übrige Familie: LINKS-Register
- ohne Verbindung: Selbstprüfung über interne MAC-Schleife (HLREG0.LPBK + MACC.FLU)

Prüfung: `tools/kern-test/vfio-e610.sh` auf server3 (QEMU/KVM + VFIO, freier Port ad:00.0).
Status 22.11.2026: bestanden (Rücksetzen, MAC, ACI, Schleife Senden/Empfangen). Offen: Verbindung mit Gegenstelle.
