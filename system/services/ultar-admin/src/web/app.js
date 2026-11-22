'use strict';
// Ultar OS – Web-Verwaltung. Ziffern immer 0–9 (auch im Arabischen).
const T = {
  ar: {title:'Ultar OS', login_sub:'إدارة الجهاز – أدخل كلمة المرور', login:'دخول', wrong:'كلمة المرور خاطئة', locked:'محاولات كثيرة – انتظر {s} ثانية', logout:'خروج',
    t_overview:'نظرة عامة', t_services:'الخدمات', t_apps:'التطبيقات', t_system:'النظام', net:'الشبكة', disks:'التخزين', cpu:'المعالج', mem:'الذاكرة', uptime:'مدة التشغيل', kernel:'النواة', battery:'البطارية',
    svc_name:'الخدمة', svc_state:'الحالة', svc_restarts:'إعادة تشغيل', svc_up:'مدة', start:'تشغيل', stop:'إيقاف', restart:'إعادة', log:'السجل',
    install:'تثبيت تطبيق', install_sub:'يدعم: ‎.uap (موقّع من KM Code Craft) و ‎.deb و ‎.apk و ‎.exe و ‎.msi و AppImage و Flatpak', drop:'اسحب الملف إلى هنا أو انقر للاختيار', silent:'تثبيت صامت (MSI بدون أسئلة)',
    installed:'التطبيقات المثبتة', none:'لا توجد تطبيقات بعد', remove:'إزالة', ubuntu:'حزم أوبونتو من المستودع', apt_install:'تثبيت', apt_remove:'إزالة',
    power:'التشغيل', reboot:'إعادة التشغيل', poweroff:'إيقاف التشغيل', confirm:'هل أنت متأكد؟', chpw:'تغيير كلمة المرور', save:'حفظ', old:'كلمة المرور الحالية', new:'كلمة المرور الجديدة (10 أحرف على الأقل)', new2:'تأكيد كلمة المرور',
    pw_ok:'تم الحفظ – سجّل الدخول من جديد', pw_short:'10 أحرف على الأقل', pw_mismatch:'كلمتا المرور غير متطابقتين', about:'حول', initial_pw:'ما زالت كلمة المرور الأولية مستخدمة – غيّرها الآن في «النظام».',
    ssh:'مفاتيح SSH', ssh_sub:'الدخول عبر SSH بالمفتاح فقط. مفتاح عام واحد في كل سطر.', ssh_bad:'سطر غير صالح: ', working:'جارٍ العمل …', done:'تم', pending:'بانتظار بيئة أندرويد', ed_phone:'إصدار الهاتف', ed_desktop:'إصدار الحاسوب', ed_server:'إصدار الخادم'},
  en: {title:'Ultar OS', login_sub:'Device administration – enter your password', login:'Sign in', wrong:'Wrong password', locked:'Too many attempts – wait {s} s', logout:'Sign out',
    t_overview:'Overview', t_services:'Services', t_apps:'Apps', t_system:'System', net:'Network', disks:'Storage', cpu:'CPU', mem:'Memory', uptime:'Uptime', kernel:'Kernel', battery:'Battery',
    svc_name:'Service', svc_state:'State', svc_restarts:'Restarts', svc_up:'Up', start:'Start', stop:'Stop', restart:'Restart', log:'Log',
    install:'Install an app', install_sub:'Supported: .uap (signed by KM Code Craft), .deb, .apk, .exe, .msi, AppImage, Flatpak', drop:'Drop a file here or click to choose', silent:'Silent install (MSI without questions)',
    installed:'Installed apps', none:'No apps yet', remove:'Remove', ubuntu:'Ubuntu packages from the archive', apt_install:'Install', apt_remove:'Remove',
    power:'Power', reboot:'Restart', poweroff:'Shut down', confirm:'Are you sure?', chpw:'Change password', save:'Save', old:'Current password', new:'New password (min. 10 characters)', new2:'Repeat new password',
    pw_ok:'Saved – please sign in again', pw_short:'At least 10 characters', pw_mismatch:'Passwords do not match', about:'About', initial_pw:'The initial password is still in use – change it now under “System”.',
    ssh:'SSH keys', ssh_sub:'SSH login with keys only. One public key per line.', ssh_bad:'Invalid line: ', working:'Working …', done:'Done', pending:'waiting for Android', ed_phone:'Phone edition', ed_desktop:'Desktop/laptop edition', ed_server:'Server edition'},
  de: {title:'Ultar OS', login_sub:'Geräteverwaltung – Kennwort eingeben', login:'Anmelden', wrong:'Falsches Kennwort', locked:'Zu viele Versuche – {s} s warten', logout:'Abmelden',
    t_overview:'Übersicht', t_services:'Dienste', t_apps:'Apps', t_system:'System', net:'Netzwerk', disks:'Speicher', cpu:'Prozessor', mem:'Arbeitsspeicher', uptime:'Laufzeit', kernel:'Kernel', battery:'Akku',
    svc_name:'Dienst', svc_state:'Zustand', svc_restarts:'Neustarts', svc_up:'Laufzeit', start:'Start', stop:'Stopp', restart:'Neustart', log:'Protokoll',
    install:'App installieren', install_sub:'Unterstützt: .uap (von KM Code Craft signiert), .deb, .apk, .exe, .msi, AppImage, Flatpak', drop:'Datei hierher ziehen oder klicken', silent:'Ohne Rückfragen installieren (MSI)',
    installed:'Installierte Apps', none:'Noch keine Apps', remove:'Entfernen', ubuntu:'Ubuntu-Pakete aus den Paketquellen', apt_install:'Installieren', apt_remove:'Entfernen',
    power:'Ein/Aus', reboot:'Neu starten', poweroff:'Ausschalten', confirm:'Wirklich?', chpw:'Kennwort ändern', save:'Speichern', old:'Aktuelles Kennwort', new:'Neues Kennwort (mind. 10 Zeichen)', new2:'Neues Kennwort wiederholen',
    pw_ok:'Gespeichert – bitte neu anmelden', pw_short:'Mindestens 10 Zeichen', pw_mismatch:'Kennwörter stimmen nicht überein', about:'Über', initial_pw:'Das Anfangskennwort ist noch aktiv – jetzt unter „System“ ändern.',
    ssh:'SSH-Schlüssel', ssh_sub:'SSH-Anmeldung nur mit Schlüssel. Ein öffentlicher Schlüssel pro Zeile.', ssh_bad:'Ungültige Zeile: ', working:'Läuft …', done:'Fertig', pending:'wartet auf Android', ed_phone:'Handy-Ausgabe', ed_desktop:'Desktop-/Laptop-Ausgabe', ed_server:'Server-Ausgabe'}
};
let lang = localStorage.getItem('ultar_lang') || (navigator.language || 'ar').slice(0, 2);
if (!T[lang]) lang = 'ar';
const t = (k, v) => (T[lang][k] || T.en[k] || k).replace('{s}', v);
const $ = (s) => document.querySelector(s);
const el = (tag, cls, txt) => { const e = document.createElement(tag); if (cls) e.className = cls; if (txt !== undefined) e.textContent = txt; return e; };

function applyLang() {
  document.documentElement.lang = lang;
  document.documentElement.dir = lang === 'ar' ? 'rtl' : 'ltr';
  document.querySelectorAll('[data-t]').forEach(e => e.textContent = t(e.dataset.t));
  $('#pw').placeholder = t('login_sub').split('–').pop().trim();
  $('#oldpw').placeholder = t('old'); $('#newpw').placeholder = t('new'); $('#newpw2').placeholder = t('new2');
}
document.querySelectorAll('[data-lang]').forEach(b => b.onclick = () => { lang = b.dataset.lang; localStorage.setItem('ultar_lang', lang); applyLang(); refresh(); });

async function api(path, opt = {}) {
  opt.headers = Object.assign({'X-Ultar': '1'}, opt.headers || {});
  if (opt.json !== undefined) { opt.body = JSON.stringify(opt.json); opt.headers['Content-Type'] = 'application/json'; opt.method = opt.method || 'POST'; }
  const r = await fetch(path, opt);
  let d = {}; try { d = await r.json(); } catch (_) {}
  if (r.status === 401 && path !== '/api/login' && path !== '/api/password') { show(false); throw new Error('login'); }
  if (!r.ok) { const e = new Error(d.error || r.status); e.data = d; e.status = r.status; throw e; }
  return d;
}
function toast(m) { const e = $('#toast'); e.textContent = m; e.classList.remove('hidden'); clearTimeout(toast.h); toast.h = setTimeout(() => e.classList.add('hidden'), 3500); }
const fmtB = (b) => { const u = ['B', 'KB', 'MB', 'GB', 'TB']; let i = 0; while (b >= 1024 && i < 4) { b /= 1024; i++; } return b.toFixed(i ? 1 : 0) + ' ' + u[i]; };
const fmtUp = (s) => { const d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60); return (d ? d + 'd ' : '') + h + 'h ' + m + 'm'; };

function show(main) { $('#login').classList.toggle('hidden', main); $('#main').classList.toggle('hidden', !main); if (main) refresh(); }

$('#loginform').onsubmit = async (ev) => {
  ev.preventDefault(); $('#loginerr').textContent = '';
  try { const d = await api('/api/login', {json: {password: $('#pw').value}}); $('#pw').value = ''; $('#initial').classList.toggle('hidden', !d.initial); show(true); }
  catch (e) { $('#loginerr').textContent = e.status === 429 ? t('locked', e.data.wait) : t('wrong'); }
};
$('#logout').onclick = async () => { await api('/api/logout', {method: 'POST'}).catch(() => {}); show(false); };
document.querySelectorAll('nav [data-tab]').forEach(b => b.onclick = () => {
  document.querySelectorAll('nav [data-tab]').forEach(x => x.classList.toggle('on', x === b));
  document.querySelectorAll('.tab').forEach(s => s.classList.toggle('hidden', s.id !== b.dataset.tab));
  refresh();
});
const current = () => (document.querySelector('nav .on') || {}).dataset?.tab || 'overview';

function stat(k, v, pct) {
  const s = el('div', 'stat'); s.append(el('div', 'k', k), el('div', 'v', v));
  if (pct !== undefined) { const b = el('div', 'bar'), i = el('i'); i.style.width = Math.max(0, Math.min(100, pct)) + '%'; b.append(i); s.append(b); }
  return s;
}
async function loadOverview() {
  const d = await api('/api/status');
  $('#edition').textContent = t('ed_' + d.edition) + ' · ' + d.host + ' · ' + d.version;
  const c = $('#cards'); c.replaceChildren(
    stat(t('cpu'), d.cpu.toFixed(1) + ' %', d.cpu),
    stat(t('mem'), fmtB(d.mem_total - d.mem_avail) + ' / ' + fmtB(d.mem_total), 100 * (d.mem_total - d.mem_avail) / Math.max(1, d.mem_total)),
    stat(t('uptime'), fmtUp(d.uptime)),
    stat(t('kernel'), d.kernel));
  c.lastChild.querySelector('.v').style.fontSize = '15px';
  if (d.battery) c.append(stat(t('battery'), d.battery.capacity + ' % · ' + d.battery.status, +d.battery.capacity));
  const n = $('#netlist'); n.replaceChildren();
  d.net.forEach(x => { const it = el('div', 'it'); const g = el('div', 'grow'); g.append(el('b', '', x.ip), el('small', '', x.if)); it.append(g); n.append(it); });
  const k = $('#disklist'); k.replaceChildren();
  d.disks.forEach(x => { const it = el('div', 'it'); const g = el('div', 'grow'); g.append(el('b', '', x.path), el('small', '', fmtB(x.total - x.free) + ' / ' + fmtB(x.total))); const b = el('div', 'bar'); b.style.width = '40%'; const i = el('i'); i.style.width = (100 * (x.total - x.free) / x.total) + '%'; b.append(i); it.append(g, b); k.append(it); });
}
const pill = (s) => el('span', 'pill ' + ({'läuft': 'ok', 'fertig': 'ok', 'fehler': 'bad', 'defekt': 'bad', 'backoff': 'wait', 'wartet': 'wait'}[s] || ''), s);
async function loadServices() {
  const d = await api('/api/services'); const tb = $('#svc tbody'); tb.replaceChildren();
  d.forEach(s => {
    const tr = el('tr'); tr.append(el('td', '', s.name)); const st = el('td'); st.append(pill(s.state)); tr.append(st, el('td', '', s.pid), el('td', '', s.restarts), el('td', '', s.up));
    const a = el('td'); const w = el('div', 'acts');
    for (const act of ['start', 'stop', 'restart']) { const b = el('button', '', t(act)); b.onclick = async () => { try { const r = await api(`/api/services/${encodeURIComponent(s.name)}/${act}`, {method: 'POST'}); toast(r.msg); setTimeout(loadServices, 800); } catch (e) { toast(e.message); } }; w.append(b); }
    const l = el('button', '', t('log')); l.onclick = async () => { const r = await api('/api/logs/' + encodeURIComponent(s.name)); $('#logcard').classList.remove('hidden'); $('#logtitle').textContent = t('log') + ': ' + s.name; $('#log').textContent = r.log || '—'; $('#log').scrollTop = 1e9; };
    w.append(l); a.append(w); tr.append(a); tb.append(tr);
  });
}
async function loadApps() {
  const d = await api('/api/apps'); const l = $('#applist'); l.replaceChildren();
  if (!d.apps.length) l.append(el('p', 'sub', t('none')));
  d.apps.forEach(x => {
    const it = el('div', 'it'); const g = el('div', 'grow'); g.append(el('b', '', x.name), el('small', '', x.format + (x.note ? ' · ' + x.note : '')));
    it.append(g); if (x.status === 'pending') it.append(el('span', 'pill wait', t('pending')));
    const b = el('button', 'danger', t('remove')); b.onclick = async () => { if (!confirm(t('confirm'))) return; b.disabled = true; try { await api(`/api/apps/${encodeURIComponent(x.id)}/remove`, {method: 'POST'}); toast(t('done')); } catch (e) { toast(e.message); } loadApps(); };
    it.append(b); l.append(it);
  });
}
async function upload(f) {
  const log = $('#instlog'); log.classList.remove('hidden'); log.textContent = t('working') + '\n' + f.name;
  const h = {'X-Filename': f.name}; if ($('#silent').checked) h['X-Silent'] = '1';
  try { const d = await api('/api/apps/install', {method: 'POST', body: f, headers: h}); log.textContent = (d.log || []).join('\n') + '\n✔ ' + d.name; toast(t('done')); }
  catch (e) { log.textContent = ((e.data && e.data.log) || []).join('\n') + '\n✖ ' + e.message; }
  loadApps();
}
$('#file').onchange = (e) => { if (e.target.files[0]) upload(e.target.files[0]); e.target.value = ''; };
const drop = $('#drop');
drop.ondragover = (e) => { e.preventDefault(); drop.classList.add('over'); };
drop.ondragleave = () => drop.classList.remove('over');
drop.ondrop = (e) => { e.preventDefault(); drop.classList.remove('over'); if (e.dataTransfer.files[0]) upload(e.dataTransfer.files[0]); };
async function apt(remove) {
  const p = $('#aptpkg').value.trim(); if (!p) return; const log = $('#aptlog'); log.classList.remove('hidden'); log.textContent = t('working');
  try { const d = await api('/api/apt', {json: {package: p, remove}}); log.textContent = d.log; toast(t('done')); } catch (e) { log.textContent = (e.data && e.data.log) || e.message; }
}
$('#aptform').onsubmit = (e) => { e.preventDefault(); apt(false); };
$('#aptrm').onclick = () => apt(true);
for (const a of ['reboot', 'poweroff']) $('#' + a).onclick = async () => { if (confirm(t('confirm'))) { try { const r = await api('/api/power/' + a, {method: 'POST'}); toast(r.msg); } catch (e) { toast(e.message); } } };
$('#pwform').onsubmit = async (e) => {
  e.preventDefault(); const m = $('#pwmsg');
  if ($('#newpw').value !== $('#newpw2').value) { m.textContent = t('pw_mismatch'); return; }
  try { await api('/api/password', {json: {old: $('#oldpw').value, new: $('#newpw').value}}); m.textContent = t('pw_ok'); setTimeout(() => show(false), 1500); }
  catch (er) { m.textContent = er.message === 'short' ? t('pw_short') : t('wrong'); }
};
$('#sshform').onsubmit = async (e) => {
  e.preventDefault(); const m = $('#sshmsg');
  try { const d = await api('/api/ssh-keys', {json: {keys: $('#sshkeys').value}}); m.textContent = t('done') + ' (' + d.count + ')'; }
  catch (er) { m.textContent = er.data && er.data.line ? t('ssh_bad') + er.data.line + '…' : er.message; }
};
async function loadSystem() { const d = await api('/api/ssh-keys'); if (document.activeElement !== $('#sshkeys')) $('#sshkeys').value = d.keys; }
async function refresh() {
  try { ({overview: loadOverview, services: loadServices, apps: loadApps, system: loadSystem})[current()](); } catch (_) {}
}
setInterval(() => { if (!$('#main').classList.contains('hidden') && current() !== 'apps' && current() !== 'system') refresh(); }, 5000);
applyLang();
api('/api/me').then(d => show(d.logged_in)).catch(() => show(false));
