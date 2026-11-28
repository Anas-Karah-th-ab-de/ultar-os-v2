"use strict";
// ultar.code-craft.tech – navigation, language entry, release lists (/downloads/releases.json) and the newest
// network update (/v1/update/latest). Text only through textContent; links only under /downloads/.
(function () {
  var doc = document.documentElement;
  var lang = doc.lang === "en" || doc.lang === "de" ? doc.lang : "ar";
  var PRE = { ar: "", en: "/en", de: "/de" }[lang];

  // ---- first visit of "/" picks English or German from the browser (Arabic stays the default)
  if (location.pathname === "/" && !/[?&]lang=ar/.test(location.search)) {
    var sameSite = false;
    try { sameSite = document.referrer && new URL(document.referrer).host === location.host; } catch (e) { sameSite = false; }
    var nav = ((navigator.languages && navigator.languages[0]) || navigator.language || "ar").slice(0, 2).toLowerCase();
    if (!sameSite && (nav === "en" || nav === "de")) { location.replace("/" + nav + "/"); return; }
  }

  var S = {
    ar: { get: "تنزيل", sig: "التوقيع", notes: "ملاحظات الإصدار", older: "إصدارات أقدم", none: "لا يوجد ملف منشور لهذا الاختيار حالياً.",
      novm: "لا يوجد بعد ملف VMware خاص؛ استخدم صورة التثبيت كما في الدليل.", latest: "أحدث إصدار:", read: "الملاحظات",
      installer: "صورة تثبيت USB · x86_64 · UEFI – لسطح المكتب والخادم", phoneimg: "صورة تثبيت USB بواجهة الهاتف · x86_64 · UEFI",
      a30: "حزمة Samsung Galaxy A30 ‏(SM-A305F) – تُكتب بـ Ultar Flash", vm: "آلة VMware جاهزة للتثبيت", other: "",
      g_desktop: "Desktop وServer", g_phone: "الهاتف", g_vm: "آلة افتراضية", upd_none: "لا يوجد حالياً تحديث منشور عبر الشبكة.",
      upd_err: "تعذّر السؤال الآن.", upd_v: "الإصدار ", ch: "القناة " },
    en: { get: "Download", sig: "Signature", notes: "Release notes", older: "Older releases", none: "No file is published for this choice right now.",
      novm: "No separate VMware file yet; use the installer image as described in the guide.", latest: "Newest release:", read: "Notes",
      installer: "USB installer image · x86_64 · UEFI – for desktop and server", phoneimg: "USB installer image with the phone interface · x86_64 · UEFI",
      a30: "Package for the Samsung Galaxy A30 (SM-A305F) – written with Ultar Flash", vm: "VMware machine ready for installation", other: "",
      g_desktop: "Desktop and server", g_phone: "Phone", g_vm: "Virtual machine", upd_none: "No update is published over the network right now.",
      upd_err: "Could not ask right now.", upd_v: "Version ", ch: "channel " },
    de: { get: "Herunterladen", sig: "Signatur", notes: "Hinweise zur Ausgabe", older: "Ältere Ausgaben", none: "Für diese Auswahl ist derzeit keine Datei veröffentlicht.",
      novm: "Noch keine eigene VMware-Datei; das Installationsabbild wie in der Anleitung verwenden.", latest: "Neueste Ausgabe:", read: "Hinweise",
      installer: "USB-Installationsabbild · x86_64 · UEFI – für Desktop und Server", phoneimg: "USB-Installationsabbild mit Handy-Oberfläche · x86_64 · UEFI",
      a30: "Paket für das Samsung Galaxy A30 (SM-A305F) – mit Ultar Flash schreiben", vm: "VMware-Maschine zur Installation", other: "",
      g_desktop: "Desktop und Server", g_phone: "Handy", g_vm: "Virtuelle Maschine", upd_none: "Derzeit ist keine Aktualisierung über das Netz veröffentlicht.",
      upd_err: "Abfrage gerade nicht möglich.", upd_v: "Version ", ch: "Kanal " }
  }[lang];

  // ---- navigation: one open menu at a time, Escape and outside click close, phone menu button
  var top = document.querySelector(".top");
  var menus = document.querySelectorAll(".nav details");
  function closeAll(except) { for (var i = 0; i < menus.length; i++) if (menus[i] !== except) menus[i].open = false; }
  for (var i = 0; i < menus.length; i++) menus[i].addEventListener("toggle", function () { if (this.open) closeAll(this); });
  document.addEventListener("click", function (e) { if (!e.target.closest || !e.target.closest(".nav details")) closeAll(null); });
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") { closeAll(null); if (top) { top.classList.remove("open"); setBtn(false); } }
  });
  var btn = document.querySelector(".menu-btn");
  function setBtn(v) { if (btn) btn.setAttribute("aria-expanded", v ? "true" : "false"); }
  if (btn) btn.addEventListener("click", function () { var o = top.classList.toggle("open"); setBtn(o); });

  // ---- helpers
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = String(text);
    return e;
  }
  function size(n) { n = Number(n) || 0; return n >= 1048576 ? (n / 1048576).toFixed(1) + " MB" : Math.ceil(n / 1024) + " KB"; }
  function safeUrl(u) { return typeof u === "string" && /^\/downloads\/[A-Za-z0-9._\-\/]+$/.test(u) && u.indexOf("..") < 0 ? u : null; }
  function kind(f, r) {
    var n = String(f.name || f.url || "");
    if (/a30/i.test(n)) return "a30";
    if (/vmware|\.ova$|\.vmdk|\.vmx/i.test(n)) return "vm";
    if (/-phone-/i.test(n) || /-phone$/.test(String(r.version || ""))) return "phoneimg";
    if (/installer.*\.img(\.xz)?$/i.test(n)) return "installer";
    return "other";
  }
  var WANT = { desktop: ["installer"], server: ["installer"], phone: ["phoneimg", "a30"], vm: ["vm"] };

  function fileItem(f, r) {
    var url = safeUrl(f.url);
    if (!url) return null;
    var li = el("li");
    li.appendChild(el("span", "fname", f.name || url.split("/").pop()));
    var d = S[kind(f, r)];
    if (d) li.appendChild(el("span", "fdesc", d));
    var acts = el("div", "acts");
    var a = el("a", "btn primary small", S.get + " · " + size(f.size));
    a.href = url; a.setAttribute("download", "");
    acts.appendChild(a);
    var sig = safeUrl(f.sig);
    if (sig) { var s = el("a", "btn small", S.sig + " (.sig)"); s.href = sig; s.setAttribute("download", ""); acts.appendChild(s); }
    li.appendChild(acts);
    if (f.sha256 && /^[0-9a-f]{64}$/i.test(f.sha256)) li.appendChild(el("code", "sha", "SHA-256 " + f.sha256));
    return li;
  }

  function card(r, files, opts) {
    var c = el("article", "rel");
    var h = el("h3");
    h.appendChild(el("span", null, (opts && opts.label ? opts.label + " · " : "") + "Ultar OS " + (r.version || "")));
    var meta = el("span");
    if (r.channel) meta.appendChild(el("span", "chip " + String(r.channel).replace(/[^a-z]/g, ""), r.channel));
    if (r.date) { meta.appendChild(document.createTextNode(" ")); var t = el("time", "chip", r.date); t.setAttribute("datetime", r.date); meta.appendChild(t); }
    h.appendChild(meta);
    c.appendChild(h);
    var notes = r.notes && (r.notes[lang] || r.notes.ar || r.notes.en);
    if (notes) {
      if (opts && opts.foldNotes) {
        var dn = el("details", "older"); dn.appendChild(el("summary", null, S.notes)); dn.appendChild(el("p", "notes", notes)); c.appendChild(dn);
      } else c.appendChild(el("p", "notes", notes));
    }
    var ul = el("ul", "files");
    files.forEach(function (f) { var li = fileItem(f, r); if (li) ul.appendChild(li); });
    c.appendChild(ul);
    return c;
  }

  function pick(list, kinds) {
    var out = [];
    list.forEach(function (r) {
      var fs = (r.files || []).filter(function (f) { return kinds.indexOf(kind(f, r)) >= 0; });
      if (fs.length) out.push({ r: r, files: fs });
    });
    return out;
  }

  function renderBox(box, list) {
    var what = box.getAttribute("data-releases");
    box.textContent = "";
    if (what === "all") {
      list.forEach(function (r) { box.appendChild(card(r, r.files || [])); });
      if (!list.length) box.appendChild(el("p", "empty", S.none));
      return;
    }
    if (what === "latest") {
      [["g_desktop", WANT.desktop], ["g_phone", WANT.phone], ["g_vm", WANT.vm]].forEach(function (g) {
        var found = pick(list, g[1]);
        if (found.length) box.appendChild(card(found[0].r, found[0].files, { label: S[g[0]], foldNotes: true }));
      });
      if (!box.firstChild) box.appendChild(el("p", "empty", S.none));
      return;
    }
    var found = pick(list, WANT[what] || []);
    if (!found.length) { box.appendChild(el("p", "empty", what === "vm" ? S.novm : S.none)); return; }
    box.appendChild(card(found[0].r, found[0].files));
    if (found.length > 1) {
      var d = el("details", "older"); d.appendChild(el("summary", null, S.older));
      var inner = el("div", "releases");
      found.slice(1).forEach(function (x) { inner.appendChild(card(x.r, x.files, { foldNotes: true })); });
      d.appendChild(inner); box.appendChild(d);
    }
  }

  function renderLatest(node, list) {
    var found = pick(list, WANT.desktop);
    var r = found.length ? found[0].r : list[0];
    if (!r) { node.hidden = true; return; }
    node.textContent = "";
    node.appendChild(document.createTextNode(S.latest + " Ultar OS " + (r.version || "") + (r.channel ? " (" + r.channel + ")" : "") + " · "));
    var a = el("a", null, S.read); a.href = PRE + "/releases/"; node.appendChild(a);
  }

  var boxes = document.querySelectorAll("[data-releases]");
  var latest = document.querySelector("[data-latest]");
  if (boxes.length || latest) {
    fetch("/downloads/releases.json", { cache: "no-store", credentials: "omit" })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (d) {
        var list = d && Array.isArray(d.releases) ? d.releases : [];
        for (var i = 0; i < boxes.length; i++) renderBox(boxes[i], list);
        if (latest) renderLatest(latest, list);
      })
      .catch(function () { if (latest) latest.hidden = true; });
  }

  // ---- newest system update over the network
  var upd = document.querySelector("[data-update-v]");
  if (upd) {
    fetch("/v1/update/latest?channel=stable&arch=x86_64&edition=desktop", { cache: "no-store", credentials: "omit", headers: { Accept: "application/json" } })
      .then(function (r) {
        if (r.status === 204) { upd.textContent = S.upd_none; return null; }
        return r.ok ? r.json() : Promise.reject(r.status);
      })
      .then(function (j) {
        if (!j) return;
        if (!j.version) { upd.textContent = S.upd_none; return; }
        upd.textContent = S.upd_v + j.version + (j.channel ? " · " + S.ch + j.channel : "") + (j.size ? " · " + size(j.size) : "");
      })
      .catch(function () { upd.textContent = S.upd_err; });
  }
})();
