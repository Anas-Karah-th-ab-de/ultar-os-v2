# Integration: Android-Kaefig in die Ultar-Oberflaeche

- Neuer Einstellungen-Screen „Android-Apps (Kaefig)": Status (androidcheck/ultar-android.sh status),
  Schalter „Kaefig aktiv" (Default AUS), Buttons Start/Stopp, „Alles loeschen" (wipe), APK installieren.
  Zeigt das Sicherheitsmodell kurz an (isoliert, abschaltbar, Restrisiko gemeinsamer Kernel).
- Start ruft /usr/lib/ultar/android/ultar-android.sh start (ueber das vorhandene launch/web-session-
  Muster bzw. einen Dienst). Android-Apps erscheinen als Home-Kacheln, die den Kaefig + die App im
  Weston-Vollbild oeffnen.
- MUSS an privacy koppeln: vor Start/Geraete-Durchreichung crate::privacy::allowed() bzw. der Container
  liest /run/ultar/privacy (schon umgesetzt in ultar-android.sh).
- i18n-Schluessel (en/de/ar): "Android apps (sandbox)","Sandbox active","Start sandbox","Stop sandbox",
  "Wipe sandbox","Install APK","Isolated and off by default".
- KEINE Aenderung an app.rs/main.rs/i18n.rs durch Agenten – Koordinator fuegt Screen + Nav zentral ein.
