"use strict";
// Ultar OS website: language switch (Arabic is the text in index.html) and the release list
// from /downloads/releases.json, rendered with textContent only; links only under /downloads/.
(function () {
  var T = {
    en: {
      badge: "Ultar OS 0.8 beta · own kernel written in Rust",
      h1: "An independent operating system, Arabic first.",
      lead: "Ultar OS is built from the ground up by KM Code Craft: its own kernel written in Rust that runs without Linux, a desktop and a phone interface, a signed app store, and every app in an isolated cell with the fewest rights.",
      cta1: "Download and install", cta2: "What works today?", cta3: "Privacy and security",
      f1: "own kernel and every system part, no Linux", f2: "until the kernel is ready in the emulator", f3: "automated tests pass without failures", f4: "Arabic right-to-left everywhere",
      feat: "What Ultar OS is today",
      a1: "Its own kernel", a1p: "The Ultar kernel is written in Rust: memory limits per group, real users and rights, process isolation, and its own UEFI loader with two A/B copies that falls back to the previous one when a start fails.",
      a2: "Desktop and phone", a2p: "Its own compositor, bar, menus and overview, a lock screen, and a phone interface. The system adapts to the device: touch, battery, a second screen.",
      a3: "Ultar Store", a3p: "A signed catalogue from the store server over HTTPS, from a USB stick or from the system itself. Every package is checked twice before it is installed, and the store keeps working offline with the last trusted catalogue.",
      a4: "Every app in its own cell", a4p: "Every app is its own process with its own user and data folder, and gets only the rights its signed manifest declares and you allowed.",
      a5: "Safe updates", a5p: "System updates are signed, protected against going back to older versions, installed into the second A/B copy with automatic fallback, and available offline on USB.",
      a6: "Arabic first", a6p: "Every text is written in Arabic first: the six Arabic plural forms, the Hijri calendar, and search that ignores diacritics and spelling variants. Digits are always 0–9.",
      look: "Screenshots of the real system", lookl: "Real screenshots of Ultar OS 0.8 in a virtual machine (QEMU), not designs.",
      s1: "The desktop after start: bar, search, apps, language and clock (Arabic session)",
      st: "Honest status: beta", stl: "Ultar OS 0.8 is a beta to try in a virtual machine or on a test device, not yet for your everyday computer.",
      ok: "Works", ok1: "Start via UEFI on PCs, in VMware and QEMU", ok2: "The desktop session: compositor, bar, lock screen, system dialogs", ok3: "Store, Settings and Files as separate apps, and signed packages from the store server", ok4: "Signed system updates on the two A/B copies with automatic fallback",
      no: "Not yet", no1: "Web engine: the own browser is not ready yet", no2: "Wi-Fi drivers for real hardware", no5: "The graphical installer is still being finished", no3: "Disk encryption: the installer option stays off until the kernel can unlock an encrypted disk at start", no4: "The phone version is not part of this release, and starting directly on real hardware is still being tested",
      sec: "Privacy and security", secl: "Your device does not trust our server: it checks everything it installs itself.",
      c1: "Apps, the catalogue and system images are signed with three separate keys (system, system apps, store). On this server they are held by a separate signing program with its own user that signs only digests in a fixed format, with a limit on the number of signatures, and logs every signature.",
      c2: "The device checks every signature against the trusted keys built into the system and refuses anything unsigned or signed with a revoked key – whatever the server says.",
      c3: "The server has no remote access to devices: no commands, no control, no silent installs.",
      c4: "This page has no cookies, no tracking, no third-party fonts or scripts, and we do not store visitors' IP addresses.",
      c5: "Every download comes with a SHA-256 fingerprint and an Ed25519 signature you can verify yourself.",
      dl: "Download and install", dl0: "The 0.8 system image is being finished and will appear here soon with its fingerprint and signature.",
      vh: "How do I verify a file?", vp1: "1. Fingerprint: compute SHA-256 and compare it with the value shown here.",
      vp2: "2. Signature: download the .sig file and the key file ultar-allowed-signers, then verify with ssh-keygen, which Linux, macOS and Windows 10/11 already have:",
      keys: "Key file ultar-allowed-signers",
      gv: "Install in VMware", gv1: "Download UltarOS-v2-…zip and check its fingerprint and signature.", gv2: "Unzip it and open UltarOS-v2.vmx in VMware Workstation or VMware Player (version 17 or later).", gv3: "The machine is prepared: UEFI, 4 GB memory, NVMe disk, NAT network. Just start it.", gv4: "The first login details are in the release notes above.",
      gu: "Install on a PC via USB", gu1: "Download the USB image (.img) and check its fingerprint and signature.", gu2: "Write it to a USB stick (at least 8 GB) as a raw image, e.g. with dd on Linux or a raw image writer. Everything on the stick is erased.", gu3: "Start the PC from the stick through the UEFI boot menu. Turn Secure Boot off – the loader is not signed for it yet.", gu4: "The installer writes the system to an empty disk and erases everything on it – use a test device or an empty disk.",
      by: "developed by", file: "File", size: "Size", get: "Download", sig: "Signature", notes: "Notes"
    },
    de: {
      badge: "Ultar OS 0.8 Beta · eigener Kern in Rust",
      h1: "Ein unabhängiges Betriebssystem, Arabisch zuerst.",
      lead: "Ultar OS wird bei KM Code Craft von Grund auf gebaut: ein eigener Kern in Rust, der ohne Linux läuft, eine Desktop- und eine Handy-Oberfläche, ein signierter App-Store, und jede App in einer abgeschotteten Zelle mit den wenigsten Rechten.",
      cta1: "Herunterladen und installieren", cta2: "Was funktioniert heute?", cta3: "Datenschutz und Sicherheit",
      f1: "eigener Kern und alle Systemteile, ohne Linux", f2: "bis der Kern im Emulator bereit ist", f3: "automatische Tests ohne Fehler", f4: "Arabisch von rechts nach links, überall",
      feat: "Was Ultar OS heute ist",
      a1: "Eigener Kern", a1p: "Der Ultar-Kern ist in Rust geschrieben: Speichergrenzen je Gruppe, echte Benutzer und Rechte, abgeschottete Prozesse und ein eigener UEFI-Lader mit zwei A/B-Kopien, der bei einem misslungenen Start zur vorigen zurückkehrt.",
      a2: "Desktop und Handy", a2p: "Eigener Compositor, Leiste, Menüs und Übersicht, Sperrbildschirm und eine Handy-Oberfläche. Das System passt sich dem Gerät an: Touch, Akku, zweiter Bildschirm.",
      a3: "Ultar Store", a3p: "Ein signierter Katalog vom Store-Server über HTTPS, von einem USB-Stick oder aus dem System selbst. Jedes Paket wird vor der Installation zweimal geprüft, und der Store arbeitet offline mit dem letzten vertrauenswürdigen Katalog weiter.",
      a4: "Jede App in ihrer Zelle", a4p: "Jede App ist ein eigener Prozess mit eigenem Benutzer und eigenem Datenordner und bekommt nur die Rechte, die ihr signiertes Manifest nennt und die Sie erlaubt haben.",
      a5: "Sichere Updates", a5p: "System-Updates sind signiert, gegen die Rückkehr zu älteren Versionen geschützt, werden in die zweite A/B-Kopie mit automatischem Rückfall installiert und gibt es offline auf USB.",
      a6: "Arabisch zuerst", a6p: "Jeder Text entsteht zuerst auf Arabisch: die sechs arabischen Pluralformen, der Hidschra-Kalender und eine Suche, die Vokalzeichen und Schreibvarianten ignoriert. Ziffern immer 0–9.",
      look: "Bilder vom echten System", lookl: "Echte Bildschirmfotos von Ultar OS 0.8 in einer virtuellen Maschine (QEMU), keine Entwürfe.",
      s1: "Der Desktop nach dem Start: Leiste, Suche, Apps, Sprache und Uhr (arabische Sitzung)",
      st: "Ehrlicher Stand: Beta", stl: "Ultar OS 0.8 ist eine Beta zum Ausprobieren in einer virtuellen Maschine oder auf einem Testgerät, noch nicht für den Alltagsrechner.",
      ok: "Funktioniert", ok1: "Start über UEFI auf PCs, in VMware und QEMU", ok2: "Die Desktop-Sitzung: Compositor, Leiste, Sperrbildschirm, Systemdialoge", ok3: "Store, Einstellungen und Dateien als eigene Apps, signierte Pakete vom Store-Server", ok4: "Signierte System-Updates auf den zwei A/B-Kopien mit automatischem Rückfall",
      no: "Noch nicht", no1: "Web-Engine: der eigene Browser ist noch nicht fertig", no2: "WLAN-Treiber für echte Hardware", no5: "Das grafische Installationsprogramm wird noch fertiggestellt", no3: "Festplattenverschlüsselung: die Option im Installer bleibt aus, bis der Kern eine verschlüsselte Platte beim Start öffnen kann", no4: "Die Handy-Version ist nicht Teil dieser Ausgabe, und der direkte Start auf echter Hardware wird noch getestet",
      sec: "Datenschutz und Sicherheit", secl: "Ihr Gerät vertraut unserem Server nicht: Es prüft alles, was es installiert, selbst.",
      c1: "Apps, Katalog und Systemabbilder sind mit drei getrennten Schlüsseln signiert (System, System-Apps, Store). Auf diesem Server verwahrt sie ein eigenes Signierprogramm mit eigenem Benutzer, das nur Prüfsummen in festem Format signiert, die Anzahl der Signaturen begrenzt und jede Signatur protokolliert.",
      c2: "Das Gerät prüft jede Signatur gegen die im System eingebauten vertrauenswürdigen Schlüssel und lehnt alles Unsignierte oder mit einem widerrufenen Schlüssel Signierte ab – egal, was der Server sagt.",
      c3: "Der Server hat keinen Fernzugriff auf Geräte: keine Befehle, keine Steuerung, keine stillen Installationen.",
      c4: "Diese Seite hat keine Cookies, kein Tracking, keine fremden Schriften oder Skripte, und wir speichern keine IP-Adressen der Besucher.",
      c5: "Jeder Download kommt mit SHA-256-Fingerabdruck und Ed25519-Signatur, die Sie selbst prüfen können.",
      dl: "Herunterladen und installieren", dl0: "Das Systemabbild 0.8 wird fertiggestellt und erscheint bald hier mit Fingerabdruck und Signatur.",
      vh: "Wie prüfe ich eine Datei?", vp1: "1. Fingerabdruck: SHA-256 berechnen und mit dem hier angezeigten Wert vergleichen.",
      vp2: "2. Signatur: die .sig-Datei und die Schlüsseldatei ultar-allowed-signers herunterladen und mit ssh-keygen prüfen, das Linux, macOS und Windows 10/11 bereits mitbringen:",
      keys: "Schlüsseldatei ultar-allowed-signers",
      gv: "Installation in VMware", gv1: "UltarOS-v2-…zip herunterladen und Fingerabdruck und Signatur prüfen.", gv2: "Entpacken und UltarOS-v2.vmx in VMware Workstation oder VMware Player (Version 17 oder neuer) öffnen.", gv3: "Die Maschine ist vorbereitet: UEFI, 4 GB Speicher, NVMe-Platte, NAT-Netz. Einfach starten.", gv4: "Die Anmeldedaten für den ersten Start stehen in den Hinweisen der Version oben.",
      gu: "Installation auf einem PC per USB", gu1: "Das USB-Abbild (.img) herunterladen und Fingerabdruck und Signatur prüfen.", gu2: "Als Rohabbild auf einen USB-Stick (mindestens 8 GB) schreiben, z. B. mit dd unter Linux oder einem Programm für Rohabbilder. Alles auf dem Stick wird gelöscht.", gu3: "Den PC über das UEFI-Startmenü vom Stick starten. Secure Boot ausschalten – der Lader ist dafür noch nicht signiert.", gu4: "Das Installationsprogramm schreibt das System auf eine leere Platte und löscht alles darauf – ein Testgerät oder eine leere Platte verwenden.",
      by: "entwickelt von", file: "Datei", size: "Größe", get: "Herunterladen", sig: "Signatur", notes: "Hinweise"
    }
  };
  var AR = { file: "الملف", size: "الحجم", get: "تنزيل", sig: "التوقيع", notes: "ملاحظات" };
  var nodes = document.querySelectorAll("[data-t]");
  for (var i = 0; i < nodes.length; i++) AR[nodes[i].getAttribute("data-t")] = nodes[i].textContent;
  T.ar = AR;
  var lang = "ar", rel = null;

  function t(k) { return (T[lang] && T[lang][k]) || AR[k] || ""; }

  function setLang(l) {
    if (!T[l]) l = "ar";
    lang = l;
    document.documentElement.lang = l;
    document.documentElement.dir = l === "ar" ? "rtl" : "ltr";
    for (var i = 0; i < nodes.length; i++) nodes[i].textContent = t(nodes[i].getAttribute("data-t"));
    var b = document.querySelectorAll(".lang button");
    for (var j = 0; j < b.length; j++) b[j].setAttribute("aria-pressed", b[j].getAttribute("data-lang") === l ? "true" : "false");
    if (rel) render(rel);
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = String(text);
    return e;
  }
  function size(n) { n = Number(n) || 0; return n > 1048576 ? (n / 1048576).toFixed(1) + " MB" : Math.ceil(n / 1024) + " KB"; }
  function safeUrl(u) { return typeof u === "string" && /^\/downloads\/[A-Za-z0-9._\-\/]+$/.test(u) && u.indexOf("..") < 0 ? u : null; }

  function fileItem(f) {
    var url = safeUrl(f.url);
    if (!url) return null;
    var li = el("li");
    li.appendChild(el("b", null, f.name || url.split("/").pop()));
    li.appendChild(el("span", "muted", size(f.size)));
    var a = el("a", "btn primary", t("get"));
    a.href = url; a.setAttribute("download", "");
    li.appendChild(a);
    var sig = safeUrl(f.sig);
    if (sig) { var s = el("a", "btn", t("sig")); s.href = sig; s.setAttribute("download", ""); li.appendChild(s); }
    if (f.sha256) li.appendChild(el("code", null, "SHA-256: " + f.sha256));
    return li;
  }

  function render(data) {
    var box = document.getElementById("releases");
    var list = (data && Array.isArray(data.releases)) ? data.releases : [];
    if (!list.length) return;
    box.textContent = "";
    list.forEach(function (r) {
      var c = el("article", "rel");
      var h = el("h3");
      h.appendChild(el("span", null, "Ultar OS " + (r.version || "")));
      h.appendChild(el("span", "muted", [r.channel, r.date].filter(Boolean).join(" · ")));
      c.appendChild(h);
      var notes = r.notes && (r.notes[lang] || r.notes.ar || r.notes.en);
      if (notes) c.appendChild(el("p", "muted", notes));
      var ul = el("ul");
      (r.files || []).forEach(function (f) { var li = fileItem(f); if (li) ul.appendChild(li); });
      c.appendChild(ul);
      box.appendChild(c);
    });
  }

  var bs = document.querySelectorAll(".lang button");
  for (var k = 0; k < bs.length; k++) bs[k].addEventListener("click", function () { setLang(this.getAttribute("data-lang")); });
  var nl = (new URLSearchParams(location.search).get("lang") || navigator.language || "ar").slice(0, 2);
  setLang(T[nl] ? nl : "ar");

  fetch("/downloads/releases.json", { cache: "no-store", credentials: "omit" })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (d) { rel = d; render(d); })
    .catch(function () {});
})();
