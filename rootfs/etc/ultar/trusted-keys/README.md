# Vertrauenswürdige Herausgeber-Schlüssel (Ultar-Store)

Hier liegen die ÖFFENTLICHEN ed25519-Schlüssel (hex, eine Zeile je Datei, Endung .ed25519), gegen die
das Gerät App-Signaturen prüft. Nur Pakete, die mit einem dieser Schlüssel signiert sind, lassen sich
installieren. Fehlt jeder Schlüssel, wird jede Installation abgelehnt.

Schlüssel erzeugen (auf dem Server/Entwicklerrechner, privaten Teil GEHEIM halten):
    upkg-keygen
Öffentlichen Teil hier als z. B. kmcodecraft.ed25519 ablegen; mit dem privaten Teil signiert
`upkg-sign` die .uap-Pakete. Siehe docs/A30-APP-PAKET.md.
