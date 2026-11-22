# Ultar-Anrufe & Video (Gerät ↔ Gerät über die Cloud)

Zwei Ultar-Geräte rufen sich über ihre **eindeutige Nummer** (UOS-XXXX-XXXX) an. Der Server
`ultar.code-craft.tech` ist nur **Vermittler** (Signalisierung); die Medien gehen Ende-zu-Ende.

## Teile
- **Server:** `/v1/signal` (WebSocket). Nach Token-Auth meldet sich jedes Gerät mit seiner Nummer
  an; danach werden `{to, type:offer|answer|ice|bye, data}`-Frames an die Zielnummer weitergereicht.
  Der Absender wird serverseitig echt eingetragen (kein Spoofing). TURN-Zugangsdaten über
  `/v1/turn-cred` (coturn).
- **Gerät:** Crate `ultar-call`.
  - Standard: `MockCall` (Zustandsmaschine, kein Netz) — die Telefon-Oberfläche läuft vollständig.
  - Feature `real`: `WebrtcCall` — echter WebSocket-Client (tokio-tungstenite) macht die komplette
    Signalisierung (Anmelden, Klingeln, Annehmen, Auflegen, Erreichbarkeit).

## Medien-Engine (verbleibender Geräteschritt)
Die SDP/ICE, die `real.rs` als Platzhalter erzeugt/verarbeitet, kommen final aus einer
`RTCPeerConnection` (Crate `webrtc`, DTLS-SRTP, Opus-Audio, VP8/H264-Video). Einhängepunkte sind in
`real.rs` markiert (`offer`/`answer`/`ice`). Audio nutzt den Geräte-Pfad (COD3035/SMA1301, siehe
A30-AUDIO-*), Video die Kamera (A30-KAMERA). Das wird erst **auf dem Gerät** gegen den Audio-Stack
verifiziert — bewusst nicht im Sandkasten „fertig gemeldet", weil es ohne echte Hardware nicht
ehrlich testbar ist.

## Ablauf eines Anrufs
```
A wählt Nummer von B → A:offer →Server→ B  (B klingelt)
B nimmt an           → B:answer →Server→ A  (beide Active)
beide                → ice-Kandidaten übers Signal, dann direkte/TURN-Medienstrecke
auflegen             → bye →Server→ Gegenstelle (Ended)
```
