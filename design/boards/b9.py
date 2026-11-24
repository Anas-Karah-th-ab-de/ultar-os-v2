# Mesh (device link): SettingsMesh (dark Arabic, light English), MeshPairShow, MeshPairJoin, MeshDevice,
# MeshIncoming (the question on the receiving device), MeshSend (the choice on the sending device), MeshStates,
# and the compact class: CompactMeshSend, CompactMeshIncoming. Only bricks of gen.py, only tokens.
# Contract: system/core/ultar-bus/src/messages/mesh.rs · notes: docs/ui/mesh.md
from gen import *
from b1 import files_window, desk_icons
from b3 import settings
from b8 import statusbar, screen, apptitle, gesturebar, PW, PH
W, H = 360, 225

def mono(s, size=None):
    return f'<span style="font-family: {FF_MONO}; font-size: {size or f12}">{ltr(s)}</span>'
def lead(icon):
    return f'<span style="width: {s7}; height: {s7}; flex: none; border-radius: {r_round}; background: {t.raised}; border: {HAIR} solid {t.ctl}; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center">{ic(icon)}</span>'
def peer(icon, name, text, end, last=False):
    return (f'<li style="min-height: {ROW_MIN}; padding: {s2} {s4}; box-sizing: border-box; display: flex; align-items: center; gap: {s3}; border-bottom: {"none" if last else HAIR + " solid " + t.line}">'
            f'<span style="display: inline-flex; color: {t.mute}">{ic(icon)}</span><div style="flex: 1; min-width: 0"><div style="font-weight: {w5}">{tx(name)}</div>{sub(text)}</div>{end}</li>')

# ---------- settings page ----------
def mesh_page():
    ar = cx.rtl
    on = row(L('الترابط', 'Mesh'), L('أجهزتك تجد بعضها وتتبادل العمل والملفات والحافظة. لا شيء يصل دون أن تُسأل.', 'Your devices find each other and hand over work, files and the clipboard. Nothing arrives unasked.'), switch(True, L('الترابط', 'Mesh'), 'mesh.toggle'), icon='mesh')
    this = row(L('هذا الجهاز', 'This device'), L('ultar-pc · حاسوب محمول', 'ultar-pc · laptop'), f'<span style="display: inline-flex; align-items: center; gap: {s2}">{mono("kq4m-7xb2-pfa3-n6td-w5hz")}{ibtn(L("نسخ معرّف الجهاز", "Copy device id"), "copy", "mesh.copy")}</span>', icon='device')
    inbox = row(L('مجلد الاستلام', 'Folder for received files'), L('تُحفظ فيه الملفات بعد قبولك', 'Files are stored here after you accept'), button(L('تغيير', 'Change'), 'mesh.inbox', name=L('تغيير مجلد الاستلام', 'Change the folder for received files')), last=True, icon='folder')
    def more(name):
        return ibtn(L('الصلاحيات: ', 'Rights: ') + name, 'forward', 'mesh.device')
    paired = card(cardhead(L('أجهزتي المقترنة', 'My paired devices'), L('أُقرنت مرة واحدة برمز من ستة أرقام', 'Paired once with a six-digit code'), button(L('إقران جهاز', 'Pair a device'), 'mesh.pair', 'primary', icon='plus'))
        + '<ul role="list">'
        + peer('phone', 'KM One', L('هاتف · متصل الآن · في الشبكة المحلية', 'Phone · connected · local network'), badge(L('متصل', 'Connected'), 'ok', 'check') + more('KM One'))
        + peer('server', 'beelink', L('خادم · متصل الآن · عبر الإنترنت', 'Server · connected · over the internet'), badge(L('متصل', 'Connected'), 'ok', 'check') + more('beelink'))
        + peer('monitor', 'ultar-office', L('حاسوب مكتبي · غير متصل · آخر ظهور 20 تشرين الثاني', 'Desktop · not connected · last seen 20 Nov'), badge(L('غير متصل', 'Not connected'), 'mute', 'offline') + more('ultar-office'), True)
        + '</ul>')
    near = card(cardhead(L('أجهزة قريبة غير مقترنة', 'Nearby devices, not paired'), L('تراها لأن الترابط مفعَّل عليها. لا تستطيع شيئاً قبل الإقران.', 'You see them because Mesh is on there. They can do nothing before pairing.'))
        + '<ul role="list">' + peer('device', 'lab-board', L('لوحة تطوير', 'Development board'), button(L('إقران', 'Pair'), 'mesh.near.pair', name=L('إقران مع lab-board', 'Pair with lab-board')), True) + '</ul>')
    return h1(L('الترابط', 'Mesh')) + card(on + this + inbox) + f'<div style="display: flex; gap: {s4}; align-items: flex-start"><div style="flex: 1; min-width: 0">{paired}</div><div style="width: {g(100)}; flex: none">{near}</div></div>'

begin('SettingsMesh.dc.html')
out('Ultar OS – الترابط', W, H, settings('mesh', mesh_page()), 'Einstellungen · Verbund: an/aus, dieses Gerät, gekoppelte und nahe Geräte · dunkel · Arabisch', 9)
begin('SettingsMeshLight.dc.html', 'light', False)
out('Ultar OS – Mesh', W, H, settings('mesh', mesh_page()), 'Einstellungen · Verbund · hell · Englisch', 9)

# ---------- pairing ----------
begin('MeshPairShow.dc.html')
code = (f'<div role="group" aria-label="رمز الإقران" style="display: flex; flex-direction: column; align-items: center; gap: {s2}; padding: {s4} 0">'
        f'<div style="font-family: {FF_MONO}; font-size: {f32}; font-weight: {w7}">{ltr("482 913")}</div>'
        f'<div role="timer" style="display: flex; align-items: center; gap: {s2}; font-size: {f12}; color: {t.mute}">{ic("clock", True)}<span>{tx("صالح لمدة")}</span>{mono("04:32")}</div></div>')
show = dialog('رمز الإقران', 'اكتب هذا الرمز على الجهاز الآخر: الإعدادات ← الترابط ← إقران جهاز. يُقبل خمس محاولات فقط.', button('إلغاء', 'mesh.pair.cancel', name='إلغاء الإقران الجاري'), live=code)
out('Ultar OS – الترابط: رمز الإقران', W, H, settings('mesh', mesh_page(), overlay=show), 'Verbund · Koppeln: dieses Gerät zeigt den Code (5 min, 5 Versuche)', 9)

begin('MeshPairJoin.dc.html')
entry = f'<div style="padding: {s2} 0">{field("الرمز المعروض على lab-board", "mesh.pair.code", value="482 91", hint="ستة أرقام", direction="ltr", error="الرمز غير صحيح. بقيت 4 محاولات.")}</div>'
join = dialog('الإقران مع lab-board', 'على الجهاز الآخر: الإعدادات ← الترابط ← إقران جهاز ← إظهار الرمز.', button('إلغاء', 'mesh.pair.cancel', name='إلغاء الإقران الجاري') + button('إقران', 'mesh.pair.join', 'primary'), live=entry)
out('Ultar OS – الترابط: إدخال الرمز', W, H, settings('mesh', mesh_page(), overlay=join), 'Verbund · Koppeln: Code des anderen Geräts eingeben (Fehlerzustand: falscher Code)', 9)

# ---------- one paired device ----------
begin('MeshDevice.dc.html')
crumb = f'<div style="display: flex; align-items: center; gap: {s2}">{ibtn("رجوع إلى الترابط", "back", "mesh.back")}{h1("KM One")}</div>'
who = card(f'<div style="padding: {s4}; display: flex; align-items: center; gap: {s3}">{lead("phone")}<div style="flex: 1">{h2("KM One")}{sub("هاتف · متصل الآن · في الشبكة المحلية")}{mono("p3rd-m8ka-2vqx-h7en-c4wy")}</div>{badge("مقترن", "ok", "check")}{badge("مشفّر من طرف إلى طرف", "ok", "secure")}</div>')
RIGHTS = [('عرض متابعة العمل', 'يعرض عليك ما تعمل عليه هناك لتكمله هنا · تُسأل في كل مرة', True), ('إرسال الملفات', 'تُسأل قبل استلام أي ملف', True),
          ('مشاركة الحافظة', 'النصوص المنسوخة · كلمات المرور لا تُرسل أبداً', True), ('رؤية شاشة هذا الجهاز', 'يظهر تنبيه ما دامت الشاشة معروضة', False),
          ('التحكّم بهذا الجهاز', 'الفأرة ولوحة المفاتيح عن بُعد', False), ('الصوت', 'التشغيل والتسجيل على هذا الجهاز', False), ('استعمال القدرة الحاسوبية', 'برامج مجلد المهامّ فقط', False),
          ('إعارة النموذج اللغوي', 'يفكّر مساعد KM One بنموذج هذا الجهاز · لا يُحفظ شيء هنا', False)]
rights = card(cardhead('ما يُسمح به لـ KM One على هذا الجهاز', 'الثلاثة الأولى تُمنح عند الإقران، والباقي بيدك') + ''.join(row(a, b, switch(on, a, 'mesh.right'), last=(n == len(RIGHTS) - 1)) for n, (a, b, on) in enumerate(RIGHTS)))
forget = card(f'<div style="padding: {s4}; display: flex; align-items: center; gap: {s4}"><div style="flex: 1">{h2("إلغاء الإقران")}{sub("ينسى هذا الجهاز KM One. للعودة يلزم رمز جديد.")}</div>{button("إلغاء الإقران", "mesh.unpair", "danger", icon="trash", name="إلغاء الإقران مع KM One")}</div>')
out('Ultar OS – الترابط: KM One', W, H, settings('mesh', crumb + f'<div style="display: flex; gap: {s4}; align-items: flex-start"><div style="flex: 1; min-width: 0">{rights}</div><div style="width: {g(100)}; flex: none; display: flex; flex-direction: column; gap: {s4}">{who}{forget}</div></div>'), 'Verbund · ein gekoppeltes Gerät: Rechte je Gerät, Entkoppeln', 9)

# ---------- the question on the receiving device ----------
def what(icon, title, text):
    return f'<div style="display: flex; align-items: center; gap: {s3}; padding: {s3}; border-radius: {r_ctl}; background: {t.surface}; border: {HAIR} solid {t.line}"><span style="display: inline-flex; color: {t.mute}">{ic(icon)}</span><div style="flex: 1; min-width: 0"><div style="font-weight: {w6}">{tx(title)}</div>{sub(text)}</div></div>'
def system_line(text):
    return f'<div style="display: flex; align-items: center; gap: {s2}; font-size: {f12}; color: {t.mute}">{ic("lock", True)}<span>{tx(text)}</span></div>'
begin('MeshIncoming.dc.html')
asked = (what('kind-text', 'notes.txt', 'محرر النصوص · 1.2 MB · يُفتح في الموضع نفسه')
         + f'<div style="display: flex; gap: {s2}">{badge("جهاز مقترن", "ok", "check")}{badge("مشفّر من طرف إلى طرف", "ok", "secure")}</div>'
         + system_line('سؤال من النظام · يُرفض تلقائياً إن لم تُجب خلال دقيقة'))
question = dialog('متابعة من KM One؟', 'يريد جهازك KM One أن تكمل هنا ما كنت تعمل عليه هناك. لا يُحفظ ولا يُفتح شيء قبل قبولك.', button('رفض', 'mesh.decline') + button('قبول', 'mesh.accept', 'primary', icon='check'), live=asked)
out('Ultar OS – الترابط: سؤال الاستلام', W, H, files_window(60, 24) + desk_icons(6) + bottombar(running=(('files', 1),), active='files') + question, 'Verbund · Zielgerät fragt immer nach (Systemdialog, 60 s, sonst abgelehnt)', 10)

# ---------- the choice on the sending device ----------
def target(icon, name, text, chosen=False, state=''):
    off = state == 'disabled'
    return (f'<button role="radio" aria-checked="{"true" if chosen else "false"}"{act("mesh.send.target", name)}{" aria-disabled=\"true\"" if off else ""}{"" if chosen else " tabindex=\"-1\""} style="width: 100%; min-height: {ROW_MIN}; padding: {s2} {s3}; box-sizing: border-box; border-radius: {r_ctl}; background: {t.hover if chosen else "transparent"}; border: {RING if chosen else HAIR} solid {t.acctext if chosen else t.ctl}; display: flex; align-items: center; gap: {s3}; {state_extra(state)}">'
            f'<span style="display: inline-flex; color: {t.mute}">{ic(icon)}</span><span style="flex: 1; min-width: 0; display: block; text-align: start"><span style="display: block; font-weight: {w6}">{tx(name)}</span><span style="display: block; font-size: {f12}; color: {t.mute}">{tx(text)}</span></span>{ic("check", True) if chosen else ""}</button>')
def targets():
    return radiogroup('الجهاز المستلِم', target('phone', 'KM One', 'محرر النصوص مثبَّت هناك · يُفتح في الموضع نفسه', True) + target('server', 'beelink', 'لا يوجد محرر النصوص هناك · يُرسل الملف فقط')
                      + target('monitor', 'ultar-office', 'غير متصل الآن', state='disabled'), column=True)
begin('MeshSend.dc.html')
choose = dialog('المتابعة على جهاز آخر', 'أكثر من جهاز يستطيع استلام «notes.txt». اختر واحداً، وسيُسأل صاحبه هناك قبل الفتح.', button('إلغاء', 'mesh.send.cancel', name='إلغاء المتابعة على جهاز آخر') + button('إرسال', 'mesh.send', 'primary', icon='mesh'), live=targets())
out('Ultar OS – الترابط: اختيار الجهاز', W, H, files_window(60, 24) + desk_icons(6) + bottombar(running=(('files', 1),), active='files') + choose, 'Verbund · Sender wählt das Ziel, sobald mehrere Geräte passen (Fenstermenü → „Auf anderem Gerät fortsetzen“)', 10)

# ---------- states ----------
def mtoast(role, icon, text, action=None, a=None):
    col = {'ok': t.ok, 'info': t.info, 'warn': t.warn, 'danger': t.danger}[role]
    btn = f'<button{act(a, action)} style="height: {tgt}; padding: 0 {s2}; border-radius: {r_ctl}; color: {t.acctext}; font-weight: {w6}">{tx(action)}</button>' if action else ''
    return (f'<div role="{"alert" if role in ("warn", "danger") else "status"}" style="width: {TOAST_W}; min-height: {g(12)}; padding: {s1} {s3}; box-sizing: border-box; border-radius: {r_card}; background: {t.raised}; border: {HAIR} solid {col}; box-shadow: {t.shadow}; display: flex; align-items: center; gap: {s2}">'
            f'<span style="display: inline-flex; color: {col}">{ic(icon)}</span><span style="flex: 1">{tx(text)}</span>{btn}{ibtn("إغلاق", "close", "toast.close")}</div>')
def spanel(caption, body):
    return f'<section aria-label="{plain(caption)}" style="flex: 1; min-width: 0; display: flex; flex-direction: column; gap: {s3}"><div style="font-size: {f12}; color: {t.mute}">{tx(caption)}</div>{body}</section>'
begin('MeshStates.dc.html')
off = card(row('الترابط', 'مُطفأ: لا منفذ مفتوح، ولا يراك أي جهاز.', switch(False, 'الترابط', 'mesh.toggle'), last=True, icon='mesh'))
none = card(empty('mesh', 'لا أجهزة مقترنة بعد', 'شغّل الترابط على جهازك الآخر، ثم أقرِنه برمز من ستة أرقام.', button('إقران جهاز', 'mesh.pair', 'primary', icon='plus')))
p1 = spanel('مُطفأ', off) + spanel('مفعَّل بلا أجهزة', none)
toasts = (mtoast('info', 'ring', 'بانتظار القبول على KM One…', 'إلغاء', 'mesh.offer.cancel') + mtoast('ok', 'check', 'فُتح «notes.txt» على KM One.')
          + mtoast('warn', 'warning', 'رفض KM One المتابعة.') + mtoast('warn', 'clock', 'لم يُجب أحد على KM One خلال دقيقة.', 'إعادة المحاولة', 'mesh.retry')
          + mtoast('danger', 'error', 'انقطع الاتصال بـ KM One. لم يُحفظ هناك شيء.', 'إعادة المحاولة', 'mesh.retry') + mtoast('ok', 'download', 'وصل «report.pdf» من beelink.', 'فتح', 'mesh.received.open'))
p2 = spanel('ما يراه المرسِل والمستلِم بعد السؤال', f'<div style="display: flex; flex-direction: column; gap: {s2}">{toasts}</div>')
hints = (hint('warn', 'لا شيء في الواجهة يمكن متابعته', 'افتح ملفاً أو صفحة في تطبيق يدعم المتابعة.') + hint('info', 'لا جهاز يستطيع الاستلام الآن', 'الأجهزة المقترنة غير متصلة، أو لا تحفظ ملفات.', icon='offline')
         + hint('danger', 'تعذّر فتح المنفذ', 'برنامج آخر يستعمل منفذ الترابط. أعد المحاولة أو أعد تشغيل الجهاز.', button('إعادة المحاولة', 'mesh.retry')))
p3 = spanel('حالات أخرى', f'<div style="display: flex; flex-direction: column; gap: {s2}">{hints}</div>')
out('Ultar OS – الترابط: الحالات', W, 110, f'<div style="position: absolute; inset: 0; padding: {s4}; box-sizing: border-box; display: flex; gap: {s4}"><div style="flex: 1; min-width: 0; display: flex; flex-direction: column; gap: {s4}">{p1}</div>{p2}{p3}</div>', 'Verbund · Zustände: aus, leer, wartet, geöffnet, abgelehnt, abgelaufen, getrennt, empfangen, kein Ziel, Fehler', 10)

# ================= compact (phone) =================
def bottom_sheet(label, inner):
    return (f'<div style="position: absolute; inset: 0; background: {t.scrim}; z-index: 5"></div><section role="dialog" aria-modal="true" aria-label="{plain(label)}" style="position: absolute; inset-inline: 0; bottom: 0; z-index: 6; border-radius: {r_card} {r_card} 0 0; background: {t.raised}; border: {HAIR} solid {t.line}; box-shadow: {t.shadow}; padding: {s4} {s4} {s5}; box-sizing: border-box; display: flex; flex-direction: column; gap: {s3}">'
            f'<span aria-hidden="true" style="align-self: center; width: {s7}; height: {s1}; border-radius: {r_round}; background: {t.ctl}"></span>{inner}</section>')
def big(label, a, kind='normal', icon=None, name=None):
    return f'<div style="display: flex; min-height: {touch}">{button(label, a, kind, icon=icon, name=name, grow=True)}</div>'
def editor_behind():
    lines = ''.join(f'<div style="height: {s2}; width: {w}%; border-radius: {r_round}; background: {t.raised}"></div>' for w in (90, 72, 84, 40, 88, 64, 78, 52))
    return apptitle('notes.txt', 'mesh.back') + f'<div aria-hidden="true" style="display: flex; flex-direction: column; gap: {s3}">{lines}</div>'

begin('CompactMeshSend.dc.html')
ctargets = radiogroup('الجهاز المستلِم', target('device', 'ultar-pc', 'محرر النصوص مثبَّت هناك · يُفتح في الموضع نفسه', True) + target('server', 'beelink', 'يُرسل الملف فقط'), column=True)
send_sheet = bottom_sheet('المتابعة على جهاز آخر', h2('المتابعة على جهاز آخر', f20) + sub('اضغط مطوّلاً ثم اسحب للأعلى لفتح هذه القائمة. يُسأل صاحب الجهاز هناك قبل الفتح.') + ctargets + big('إرسال', 'mesh.send', 'primary', icon='mesh') + big('إلغاء', 'mesh.send.cancel', name='إلغاء المتابعة على جهاز آخر'))
out('Ultar OS – الهاتف: المتابعة على جهاز آخر', PW, PH, statusbar() + screen(editor_behind()) + gesturebar() + send_sheet, 'Kompakt · Weitergeben: lange drücken + nach oben wischen → Ziel wählen', 11)

begin('CompactMeshIncoming.dc.html')
casked = (what('kind-text', 'notes.txt', 'محرر النصوص · 1.2 MB · في الموضع نفسه')
          + f'<div style="display: flex; flex-wrap: wrap; gap: {s2}">{badge("جهاز مقترن", "ok", "check")}{badge("مشفّر من طرف إلى طرف", "ok", "secure")}</div>' + system_line('سؤال من النظام · يُرفض تلقائياً إن لم تُجب خلال دقيقة'))
ask_sheet = bottom_sheet('متابعة من ultar-pc؟', h2('متابعة من ultar-pc؟', f20) + sub('يريد جهازك ultar-pc أن تكمل هنا ما كنت تعمل عليه هناك. لا يُحفظ ولا يُفتح شيء قبل قبولك.') + casked + big('قبول', 'mesh.accept', 'primary', icon='check') + big('رفض', 'mesh.decline'))
out('Ultar OS – الهاتف: سؤال الاستلام', PW, PH, statusbar() + screen(apptitle('الشاشة الرئيسية')) + gesturebar() + ask_sheet, 'Kompakt · Zielgerät fragt nach (Blatt von unten, Systemfrage)', 11)
