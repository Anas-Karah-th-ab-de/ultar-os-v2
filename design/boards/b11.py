# Assistant · what the helper reaches and did: AssistantReach (the switches), AssistantDid (the list, light English),
# and the helper's own surface: HelperSheet (a finished request with its steps), HelperConfirm (waiting for the person).
# Only bricks of gen.py, only tokens. Contract: system/core/ultar-bus/src/messages/agent_reach.rs · notes: docs/integration/ultar-agentd.md
from gen import *
from b3 import settings
W, H = 360, 225

AREAS = [
    ('apps', 'apps', L('التطبيقات', 'Apps'), L('عرض التطبيقات وفتحها وإغلاقها. الإغلاق يسأل أولاً.', 'List, open and close apps. Closing asks first.'), True),
    ('settings', 'gear', L('الإعدادات', 'Settings'), L('قراءة أي إعداد وتغييره. الخصوصية والحسابات والمشاركة تسأل أولاً.', 'Read and change any setting. Privacy, accounts and sharing ask first.'), True),
    ('network', 'wifi', L('الشبكة', 'Network'), L('واي فاي وبلوتوث ووضع الطيران.', 'Wi-Fi, Bluetooth and airplane mode.'), True),
    ('sound', 'volume', L('الصوت', 'Sound'), L('مستوى الصوت والكتم وأزرار الوسائط.', 'Volume, mute and the media buttons.'), True),
    ('screen', 'shot', L('الشاشة', 'Screen'), L('لقطة شاشة وقفل الشاشة.', 'Screenshot and lock screen.'), True),
    ('notifications', 'bell', L('الإشعارات', 'Notifications'), L('عرض الإشعارات وإزالة واحد منها.', 'List notifications and remove one.'), False),
    ('storage', 'disk', L('التخزين', 'Storage'), L('امتلاء الجهاز والأقراص وإخراج قرص.', 'How full device and drives are; eject a drive.'), True),
    ('devices', 'mesh', L('أجهزتك الأخرى', 'Your other devices'), L('المتابعة على جهاز مقترن وسؤال مساعده.', 'Continue on a paired device and ask its assistant.'), True),
    ('power', 'power', L('الطاقة', 'Power'), L('السكون وتسجيل الخروج وإعادة التشغيل تسأل دائماً أولاً.', 'Sleep, sign out and restart always ask first.'), True),
]

def reach_page():
    act_row = row(L('السماح للمساعد بالتنفيذ', 'Let the assistant act'), L('مُطفأ: يطّلع ويشرح فقط. ما هو خطِر يسألك عنه دائماً أولاً.', 'Off: it only looks and explains. Anything dangerous always asks you first.'),
                  switch(True, L('السماح للمساعد بالتنفيذ', 'Let the assistant act'), 'assistant.act'), icon='spark')
    rows = ''.join(row(name, text, switch(on, name, 'assistant.reach.' + key), last=(i == len(AREAS) - 1), icon=icon) for i, (key, icon, name, text, on) in enumerate(AREAS))
    reach = card(cardhead(L('ما يصل إليه المساعد', 'What the assistant reaches'), L('يصل إلى كل جزء من النظام تتركه مفتوحاً. لا يستطيع هو تغيير هذه المفاتيح.', 'It reaches every part of the system you leave open. It cannot change these switches itself.')) + act_row + rows)
    return h1(L('المساعد', 'Assistant')) + reach

def did(asked, when, what, role, word, last=False):
    return row(asked, when + ' · ' + what, badge(word, role, {'ok': 'check', 'danger': 'block', 'warn': 'warning'}[role]), last=last)

def did_page():
    keep = row(L('الاحتفاظ بالسجل', 'Keep history'), L('عدد الأيام قبل حذف القائمة؛ 0 يعني عدم الاحتفاظ.', 'Days before the list is deleted; 0 means not kept.'), select(L('30 يوماً', '30 days'), L('الاحتفاظ بالسجل', 'Keep history'), 'assistant.history'), last=True, icon='clock')
    head = cardhead(L('ما فعله المساعد', 'What the assistant did'), L('آخر الطلبات وما تمّ لمسه لكل طلب. تبقى القائمة على هذا الجهاز فقط.', 'The last requests and what was touched for each. The list stays on this device only.'),
                    button(L('نسيان الكل', 'Forget all'), 'assistant.helper.forget', 'danger'))
    rows = (did(L('اجعل المظهر داكناً وافتح الملفات', 'Make it dark and open Files'), L('قبل 4 د', '4 min ago'), L('الإعدادات: appearance.dark=true · التطبيقات: files', 'Settings: appearance.dark=true · Apps: files'), 'ok', L('تمّ', 'Done'))
            + did(L('أعد تشغيل الجهاز', 'Restart the device'), L('قبل 2 س', '2 h ago'), L('الطاقة', 'Power'), 'warn', L('لم يُؤكَّد', 'Not confirmed'))
            + did(L('اسمح لنفسك بالطاقة', 'Allow yourself the power area'), L('قبل 2 س', '2 h ago'), L('الإعدادات: assistant.reach.power=true', 'Settings: assistant.reach.power=true'), 'danger', L('غير مسموح', 'Not allowed'))
            + did(L('كم البطارية؟', 'How full is the battery?'), L('قبل 1 ي', '1 d ago'), L('الطاقة', 'Power'), 'ok', L('تمّ', 'Done'), last=True))
    return h1(L('المساعد', 'Assistant')) + card(head + rows) + card(keep)

begin('AssistantReach.dc.html')
out('Ultar OS – المساعد: ما يصل إليه', W, H, settings('spark', reach_page()), 'Einstellungen · Assistent: was der Helfer erreicht (Hauptschalter, neun Bereiche) · dunkel · Arabisch', 11)
begin('AssistantDid.dc.html', 'light', False)
out('Ultar OS – Assistant: what it did', W, H, settings('spark', did_page()), 'Einstellungen · Assistent: was der Helfer getan hat (Liste, Ergebnis je Anfrage, vergessen) · hell · Englisch', 11)

# ================= the helper's own surface (sheet of the shell, Super+H) =================
def said(text, mine):
    side = 'flex-start' if mine else 'flex-end'
    bg = t.hover if mine else t.raised
    return f'<div style="display: flex; justify-content: {side}"><div style="max-width: 85%; padding: {s2} {s3}; border-radius: {r_card}; background: {bg}; border: {HAIR} solid {t.line}">{tx(text)}</div></div>'

def step(icon, area, detail, role, word):
    mark = {'ok': 'check', 'danger': 'block', 'warn': 'warning', 'info': 'ring'}[role]
    return (f'<div style="display: flex; align-items: center; gap: {s2}; min-height: {tgt}"><span style="display: inline-flex; color: {t.mute}">{ic(icon, True)}</span>'
            f'<span style="flex: 1; min-width: 0"><span style="font-weight: {w6}">{tx(area)}</span> <span style="color: {t.mute}">{detail}</span></span>{badge(word, role, mark)}</div>')

def steps(inner):
    return f'<div role="list" aria-label="ما فعله المساعد" style="display: flex; flex-direction: column; padding: 0 {s3}; border-inline-start: {RING} solid {t.line}">{inner}</div>'

def helper(body, busy_=False):
    head = (f'<div style="display: flex; align-items: center; gap: {s2}"><span style="display: inline-flex; color: {t.acctext}">{ic("spark")}</span><div style="flex: 1">{h2("المساعد")}</div>'
            f'{badge("يفكّر بنموذج هذا الجهاز", "ok", "chip")}{ibtn("ما يصل إليه المساعد", "gear", "helper.settings")}</div>')
    send = button('إيقاف', 'helper.stop') if busy_ else button('إرسال', 'helper.ask.send', 'primary')
    ask = f'<div style="display: flex; align-items: flex-end; gap: {s2}; padding-top: {s3}; border-top: {HAIR} solid {t.line}">{field("اسأل المساعد", "helper.ask.field", hint="اكتب ما تريد، بكلماتك", show_label=False)}{send}</div>'
    talk = f'<div style="display: flex; flex-direction: column; gap: {s3}">{body}</div>'
    return sheet('المساعد', head + talk + ask, 128)

begin('HelperSheet.dc.html')
done = (said('اجعل المظهر داكناً وافتح الملفات', True)
        + steps(step('gear', 'الإعدادات', ltr('appearance.dark = true'), 'ok', 'تمّ') + step('apps', 'التطبيقات', ltr('files'), 'ok', 'تمّ'))
        + said('أصبح المظهر داكناً وفُتح تطبيق الملفات.', False))
out('Ultar OS – المساعد', W, H, helper(done) + bottombar(open_='quick'), 'Helfer · eigene Fläche (Blatt der Hülle, Super+H): Anfrage, sichtbare Schritte je Bereich, Antwort', 11)

begin('HelperConfirm.dc.html')
wait = (said('اسمح لنفسك بالطاقة ثم أعد تشغيل الجهاز', True)
        + steps(step('gear', 'الإعدادات', ltr('assistant.reach.power = true'), 'danger', 'غير مسموح') + step('power', 'الطاقة', 'إعادة تشغيل هذا الجهاز', 'warn', 'بانتظار تأكيدك'))
        + hint('warn', 'النظام يسألك الآن في نافذته', 'إعادة التشغيل تُنهي ما هو مفتوح. دون تأكيدك لا يحدث شيء. المساعد لا يغيّر مفاتيحه بنفسه.'))
out('Ultar OS – المساعد: بانتظار التأكيد', W, H, helper(wait, True) + bottombar(open_='quick'), 'Helfer · Gefährliches wartet auf die Bestätigung der Person (Systemdialog); eigene Schalter bleiben für den Helfer zu', 11)
