# Ergebnis Chaos-Engineering Ultar-Verbund (22.11.2026)

Umgebung: Cloud-Container mit 2 CPU-Kernen (Beschleunigung hier nicht messbar, nur Korrektheit), Linux-Namespaces, ultraos-server als Relais; Ultar-Kern in QEMU (TCG) im tap-Netz; Live-Test vom PC (WSL) gegen ultar.code-craft.tech.

| Test | Ergebnis |
|---|---|
| chaos.sh (Verbund, Störungen, Sicherheit) | 17/17 |
| fern-test.sh (Bildschirm, Weitergeben, Ton, Mining, Chaos) | 26/26 |
| kern-test.sh (Ultar-Kern im Verbund) | 7/7 |
| live-test.sh (echter Server ultar.code-craft.tech) | 8/8 |

## fern-test.sh
```
OK      Gerät ohne Bildschirm: virtueller Bildschirm startet, Foto 800×600
OK      virtueller Bildschirm im Protokoll
OK      Bild hat Inhalt (nicht leer)
OK      Steuern: Einstellungen öffnen ändert das Bild
OK      Eingabe: Zurück-Taste aus der Ferne
OK      Anzeige-Steuerung: Drehen 90° (600×800)
OK      fremde Befehle als Aktion abgelehnt
OK      Helfer-Anweisung weitergeleitet
OK      Browser-Ansicht: Schlüssel, ganze Kacheln, Eingaben, Datei, Aktion
OK      Datei aus dem Browser landet sicher im Empfang
OK      Bild weitergegeben, gleich, dort geöffnet
OK      Ziel zeigt das empfangene Bild an
OK      zweite Übergabe überschreibt nichts
OK      App weitergeben: Ziel ohne Oberfläche meldet es sauber
OK      App weitergeben: Ziel öffnet die Ansicht, Quelle teilt ihren Bildschirm
OK      Mithören: Mikrofon von geraet2 kommt hier unverändert an
OK      Durchsage: hier gesprochen, dort abgespielt
OK      Ethereum ehrlich abgelehnt (Proof-of-Stake)
OK      ein Klick: Einstellungen auf allen Geräten
OK      Miner startet, wenn das Gerät ruht
OK      Bedienung stoppt den Miner sofort
OK      Verbund-Aufgabe hat Vorrang vor dem Mining
OK      fremdes Gerät sieht den Bildschirm nicht
OK      Teilen ausgeschaltet → abgelehnt
OK      Oberfläche abgeschossen → startet beim nächsten Zugriff neu
OK      Netz getrennt: Browser-Ansicht verbindet sich selbst neu (WIEDER 3.5)
```

## live-test.sh
```
OK Konto, Vertrauen, Sichtbar über Relais, Steuern (1,7 s inkl. Verbindungsaufbau), 40 Aufgaben korrekt,
OK 300-KB-Datei unverändert, Absturz mitten im Lauf: 120/120 (22 doppelt gestartet), Server: 934 Rahmen, 0 verworfen
```

## chaos.sh
```
== Ergebnis: 17 bestanden, 0 fehlgeschlagen
entdeckung     OK     Heimnetz per Rundruf, geraet5 über Relais, Konto-Geräte vertraut
grundlauf      OK     4743 ms – wiederholt 0, doppelt gestartet 0, ausgefallen [], hier nachgeholt 0
steuern        OK     Befehl lief auf geraet5 (anderes Netz) über Relais
kopplung       OK     falscher Code abgewiesen, richtiger gekoppelt
absturz        OK     4825 ms – wiederholt 8, doppelt gestartet 0, ausgefallen [], hier nachgeholt 0
trennung       OK     5319 ms – wiederholt 0, doppelt gestartet 3, ausgefallen [], hier nachgeholt 0
relais_weg     OK     4812 ms – wiederholt 8, doppelt gestartet 0, ausgefallen ["geraet5 (UV-6286-1e39)"], hier nachgeholt 0
stoerung       OK     9925 ms – wiederholt 4, doppelt gestartet 2, ausgefallen [], hier nachgeholt 0
ausschalten    OK     4911 ms – wiederholt 8, doppelt gestartet 0, ausgefallen ["geraet2 (UV-abac-ed1f)"], hier nachgeholt 0
aus_unsichtbar OK     geraet2 unsichtbar, Verbindung abgelehnt (ultar-verbund: failed to fill whole buff)
totalausfall   OK     10421 ms – wiederholt 20, doppelt gestartet 1, ausgefallen ["geraet2 (UV-abac-ed1f)", "geraet3 (UV-7b79-35d3)", "geraet5 (UV-6286-1e39)"], hier nachgeholt 0
entkoppelt     OK     entkoppeltes Gerät abgewiesen
rateschutz     OK     nach 5 Fehlversuchen ist der Code verworfen
fremdkonto     OK     fremdes Konto kann Geräteschlüssel nicht übernehmen (409)
ohne_konto     OK     ohne gültiges Token keine Geräteliste (401)
e2e            OK     Server-Log enthält keine Klartext-Inhalte
bench          OK     Beschleunigung      1.05×
```
