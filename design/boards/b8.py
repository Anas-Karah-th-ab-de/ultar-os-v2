# Round 4 (audit B5): app grid, window menu, notification banners and toasts (desktop, dark, Arabic) and the
# compact class (phone 360 x 800, dark, Arabic): home, app grid, quick settings, lock screen, settings list and
# display page, Store list and app page. Only bricks of gen.py / b2.py / b7.py, only tokens.
from gen import *
from b1 import files_window, terminal_window, desk_icons
from b2 import qt, notif, OFF, MOFF, ON
from b7 import panel, mitem, msep, tab, tablist

# ---------- new strings: key -> (Arabic, English); build.py writes them to docs/ui/CATALOG-PROPOSAL.md ----------
STR = {}
def T(key, ar, en):
    assert STR.get(key, (ar, en)) == (ar, en), 'string key reused with another text: ' + key
    STR[key] = (ar, en)
    return ar

W, H = 360, 225        # desktop boards (grid units)
PW, PH = 90, 200       # compact boards: 360 x 800 logical points

# every app that has an icon of its own (ICONS.md); names already in the catalogs (app.*)
APPS9 = [('folder', 'الملفات', 'system'), ('browser', 'المتصفح', 'web'), ('terminal', 'الطرفية', 'tools'), ('editor', 'محرر النصوص', 'tools'), ('bag', 'متجر Ultar', 'system'),
         ('gear', 'الإعدادات', 'system'), ('phone', 'الهاتف', 'comm'), ('spark', 'المساعد', 'system'), ('firefox', 'Firefox', 'web')]
def akey(app):
    """Alphabetical in Arabic order; the article ال is not counted; Latin names after the Arabic ones."""
    n = app[1]
    return (1 if n[:1].isascii() else 0, n[2:] if n.startswith('ال') else n)
ALPHA = sorted(APPS9, key=akey)
def cats():
    return [(T('shell.apps.cat.all', 'الكل', 'All'), True), (T('shell.apps.cat.web', 'الإنترنت', 'Internet'), False), (T('shell.apps.cat.comm', 'التواصل', 'Communication'), False),
            (T('shell.apps.cat.tools', 'الأدوات', 'Tools'), False), (T('shell.apps.cat.system', 'النظام', 'System'), False)]
def cat_tabs():
    return tablist(''.join(tab(n, on, a='apps.category') for n, on in cats()), T('shell.apps.categories', 'الفئات', 'Categories'))
def app_tile(icon, name, a='apps.app', lit=False, menu=False, width=None):
    """App in a grid: 48 tile with the 24 icon (ICONS.md "Open": no icon_large yet), name below in two lines at most."""
    extra = ' aria-haspopup="menu" aria-expanded="true"' if menu else ''
    w = f'width: {width}' if width else 'width: 100%'
    return (f'<button{act(a, name)}{extra} style="{w}; min-height: {g(24)}; padding: {s2} {s1}; box-sizing: border-box; border-radius: {r_ctl}; display: flex; flex-direction: column; align-items: center; gap: {s2}; background: {t.hover if lit else "transparent"}; border: {HAIR} solid {t.ctl if lit else "transparent"}">'
            f'<span style="width: {s7}; height: {s7}; flex: none; border-radius: {r_ctl}; background: {t.raised}; border: {HAIR} solid {t.line}; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center">{ic(icon)}</span>'
            f'<span style="font-size: {f12}; font-weight: {w5}; text-align: center">{tx(name)}</span></button>')
def sbox(label, a):
    """Search field in a column: keeps its own height (the brick grows along the main axis otherwise)."""
    return f'<div style="display: flex">{searchbox(label, a)}</div>'
def section(title, inner, label=None):
    return f'<section aria-label="{plain(label or title)}" style="display: flex; flex-direction: column; gap: {s2}">{h2(title, f14)}{inner}</section>'

# ================= desktop: app grid (shell.apps, Super+A) =================
begin('AppGrid.dc.html')
grid_pinned = f'<div style="display: grid; grid-template-columns: repeat(8, minmax(0, 1fr)); gap: {s2}">' + ''.join(app_tile(i, n) for k, _ in APPS for i, n, _ in APPS9 if i == k) + '</div>'
def all_grid():
    cells = ''
    for i, n, _ in ALPHA:
        cells += app_tile(i, n, lit=(i == 'browser'), menu=(i == 'browser'))
    return f'<div style="position: relative; display: grid; grid-template-columns: repeat(8, minmax(0, 1fr)); gap: {s2}">{cells}<div style="position: absolute; top: {g(25)}; inset-inline-start: {g(71)}; z-index: 6">{ctx}</div></div>'
ctx = panel(mitem(T('shell.apps.menu.open', 'فتح', 'Open'), chevrons=False, slot=False) + mitem(T('shell.apps.menu.new_window', 'نافذة جديدة', 'New window'), slot=False)
            + msep() + mitem(T('shell.apps.menu.unpin', 'إزالة من الشريط', 'Remove from the bar'), slot=False, a='apps.pin') + mitem(T('shell.apps.menu.details', 'التفاصيل في المتجر', 'Details in the Store'), slot=False, a='apps.details')
            + msep() + mitem(T('shell.apps.menu.uninstall', 'إزالة التطبيق…', 'Remove app…'), slot=False, danger=True, a='apps.uninstall'), T('shell.apps.menu', 'قائمة التطبيق: المتصفح', 'App menu: Browser'), width=56)
keys = f'<div style="font-size: {f12}; color: {t.mute}; display: flex; align-items: center; gap: {s2}">{key("Super")}{key("A")}<span>{tx(T("shell.apps.key.open", "يفتح هذه الشبكة", "opens this grid"))}</span>{key("Esc")}<span>{tx(T("shell.apps.key.close", "يغلقها", "closes it"))}</span></div>'
grid_sheet = (f'<section role="dialog" aria-label="{S()["apps"]}" style="position: absolute; top: {s6}; bottom: {g(18)}; inset-inline-start: {g(80)}; width: {g(200)}; z-index: 5; border-radius: {r_card}; background: {t.surface}; border: {HAIR} solid {t.line}; box-shadow: {t.shadow}; padding: {s5}; box-sizing: border-box; display: flex; flex-direction: column; gap: {s4}">'
              f'{sbox(T("shell.apps.search", "ابحث في التطبيقات", "Search apps"), "apps.search")}{cat_tabs()}'
              + section(T('shell.apps.pinned', 'في الشريط', 'In the bar'), grid_pinned)
              + section(S()['apps'], all_grid())
              + f'<div style="flex: 1"></div><div style="display: flex; align-items: center; justify-content: space-between; gap: {s4}">{note(T("shell.apps.note", "مرتّبة أبجدياً دون «ال». اسحب تطبيقاً فوق آخر لإنشاء مجلّد؛ تظهر الصفحات حين لا تتّسع الشبكة.", "Alphabetical, the article is not counted. Drop an app on another to make a folder; pages appear when the grid does not fit."))}{keys}</div></section>')
out('Ultar OS – كل التطبيقات', W, H, desk_icons(6) + grid_sheet + bottombar(open_='apps'), 'Hülle · Alle Apps (Super+A): Suche, Kategorien, angeheftet, Rechtsklick-Menü (§5.1)', 6)

# ================= desktop: window menu (Alt+Space, right click on the title bar) =================
begin('WindowMenu.dc.html')
wm = panel(mitem(T('shell.window_menu.move', 'نقل', 'Move'), chevrons=True, a='win.menu.move') + mitem(T('shell.window_menu.resize', 'تغيير الحجم', 'Resize'), chevrons=True, a='win.menu.resize') + msep()
           + mitem(S()['minimize'], chevrons=True, a='win.min') + mitem(S()['maximize'], shortcut=ltr('Super+↑'), chevrons=True, a='win.max') + msep()
           + mitem(T('shell.window_menu.on_top', 'دائماً في الأعلى', 'Always on top'), chevrons=True, checkable=True, a='win.menu.ontop')
           + mitem(T('shell.window_menu.all_workspaces', 'على كل مساحات العمل', 'On all workspaces'), chevrons=True, checked=True, a='win.menu.allws')
           + mitem(T('shell.window_menu.to_workspace', 'نقل إلى مساحة عمل', 'Move to workspace'), sub=True, lit=True, chevrons=True)
           + mitem(T('shell.window_menu.to_display', 'نقل إلى شاشة أخرى', 'Move to another display'), sub=True, chevrons=True) + msep()
           + mitem(S()['close'], shortcut=ltr('Alt+F4'), chevrons=True, a='win.close'), T('shell.window_menu', 'قائمة النافذة: الملفات', 'Window menu: Files'), width=64)
wsub = panel(mitem(T('shell.window_menu.ws_current', 'مساحة العمل 1 (الحالية)', 'Workspace 1 (current)'), disabled=True, slot=False, a='win.menu.workspace') + mitem('مساحة العمل 2', slot=False, a='win.menu.workspace')
             + mitem(T('shell.window_menu.ws_new', 'مساحة عمل جديدة', 'New workspace'), slot=False, a='win.menu.workspace'), T('shell.window_menu.to_workspace', 'نقل إلى مساحة عمل', 'Move to workspace'), width=56)
menus = f'<div style="position: absolute; top: {g(28)}; inset-inline-start: {g(90)}; z-index: 6; display: flex; align-items: flex-start; gap: {s1}">{wm}<div style="position: relative; top: {g(52)}">{wsub}</div></div>'
howto = (f'<div style="position: absolute; bottom: {g(18)}; inset-inline-end: {s5}; width: {g(84)}; z-index: 5; padding: {s3}; box-sizing: border-box; border-radius: {r_card}; background: {t.surface}; border: {HAIR} solid {t.line}; display: flex; flex-direction: column; gap: {s2}; font-size: {f12}; color: {t.mute}">'
         f'<div style="display: flex; align-items: center; gap: {s2}; flex-wrap: wrap">{key("Alt+Space")}<span>{tx(T("shell.window_menu.hint_open", "أو نقرة يمنى على شريط العنوان تفتح القائمة.", "or a right click on the title bar opens the menu."))}</span></div>'
         f'<div style="display: flex; align-items: center; gap: {s2}; flex-wrap: wrap"><span>{tx(T("shell.window_menu.hint_move", "نقل وتغيير الحجم: الأسهم، ثم", "Move and resize: arrow keys, then"))}</span>{key("Enter")}<span>{tx(T("shell.window_menu.hint_cancel", "للتأكيد أو", "to confirm or"))}</span>{key("Esc")}<span>{tx(T("shell.window_menu.hint_back", "للتراجع.", "to go back."))}</span></div></div>')
out('Ultar OS – قائمة النافذة', W, H, files_window(88, 18) + terminal_window(40, 132) + desk_icons(6) + menus + howto + bottombar(running=(('files', 1), ('terminal', 1)), active='files'),
    'Hülle · Fenstermenü (Alt+Leertaste / Rechtsklick Titelleiste) mit Untermenü (§5.2)', 6)

# ================= desktop: notification banners and a toast =================
begin('Toasts.dc.html')
def banner(icon, app, when, title, text, actions, important=False):
    """Notification banner (§5.3): top end, size.toast_width, raised, the one shadow; 'important' keeps its tone hairline and stays."""
    bd = t.warn if important else t.line
    tag = badge(T('shell.notification.important', 'مهمّ', 'Important'), 'warn', 'warning') if important else ''
    return (f'<li role="{"alert" if important else "status"}" style="width: {TOAST_W}; border-radius: {r_card}; background: {t.raised}; border: {HAIR} solid {bd}; box-shadow: {t.shadow}; padding: {s3}; box-sizing: border-box; display: flex; gap: {s3}">'
            f'<span style="display: inline-flex; color: {t.warn if important else t.mute}">{ic(icon)}</span><div style="flex: 1; min-width: 0; display: flex; flex-direction: column; gap: {s1}">'
            f'<div style="display: flex; align-items: center; gap: {s2}; font-size: {f12}; color: {t.mute}"><span style="font-weight: {w5}">{tx(app)}</span><span>·</span><span>{tx(when)}</span><span style="flex: 1"></span>{tag}</div>'
            f'<div style="font-weight: {w6}">{tx(title)}</div><div style="font-size: {f12}; color: {t.mute}">{tx(text)}</div><div style="display: flex; gap: {s2}; padding-top: {s1}">{actions}</div></div>'
            f'{ibtn(T("shell.notification.dismiss", "إخفاء الإشعار", "Dismiss notification") + ": " + title, "close", "note.dismiss")}</li>')
now = T('shell.notification.now', 'الآن', 'now')
b1_ = banner('phone', 'الهاتف', now, 'مكالمة فائتة من ليلى حداد', 'عبر رقم Ultar', button(T('shell.notification.call_back', 'معاودة الاتصال', 'Call back'), 'note.action', name='ليلى حداد: معاودة الاتصال') + button(T('shell.notification.message', 'رسالة', 'Message'), 'note.action', 'quiet', name='ليلى حداد: رسالة'))
b2_ = banner('usb', 'الملفات', now, 'تم توصيل ذاكرة USB', 'جاهزة للاستخدام.', button('فتح', 'note.action', name='ذاكرة USB: فتح') + button('إخراج', 'note.action', 'quiet', name='ذاكرة USB: إخراج'))
b3_ = banner('battery', 'الطاقة', now, T('shell.notification.battery_low', 'البطارية منخفضة: 9%', 'Battery low: 9%'), T('shell.notification.battery_low_text', 'وصّل الشاحن، أو شغّل وضع التوفير ليدوم الجهاز أكثر.', 'Plug in the charger, or switch to the saver mode to last longer.'),
             button(T('shell.notification.saver', 'تشغيل وضع التوفير', 'Switch to saver'), 'note.action', name='البطارية منخفضة: تشغيل وضع التوفير'), important=True)
stack = f'<ul role="list" aria-label="{T("shell.notification.banners", "إشعارات جديدة", "New notifications")}" style="position: absolute; top: {s4}; inset-inline-end: {s4}; z-index: 6; display: flex; flex-direction: column; gap: {s2}">{b3_}{b1_}{b2_}</ul>'
toast = (f'<div style="position: absolute; inset-inline: 0; bottom: {g(18)}; z-index: 6; display: flex; justify-content: center"><div role="status" style="width: {TOAST_W}; height: {g(12)}; padding: 0 {s3}; box-sizing: border-box; border-radius: {r_card}; background: {t.raised}; border: {HAIR} solid {t.ok}; box-shadow: {t.shadow}; display: flex; align-items: center; gap: {s2}">'
         f'<span style="display: inline-flex; color: {t.ok}">{ic("check")}</span><span style="flex: 1">{tx(T("files.toast.trashed", "نُقلت 3 ملفات إلى سلة المحذوفات.", "3 files moved to the Trash."))}</span>'
         f'<button{act("toast.action", T("action.undo", "تراجع", "Undo"))} style="height: {tgt}; padding: 0 {s2}; border-radius: {r_ctl}; color: {t.acctext}; font-weight: {w6}">{tx("تراجع")}</button>{ibtn(S()["close"], "close", "toast.close")}</div></div>')
legend = (f'<div style="position: absolute; bottom: {g(18)}; inset-inline-start: {s5}; width: {g(80)}; z-index: 6">'
          + note(T('shell.notification.legend', 'اللافتات في أعلى النهاية: تختفي بعد 5 ثوانٍ وتبقى تحت المؤشّر؛ «مهمّ» يبقى حتى يُغلق. الإشعار المؤقّت للتطبيق أسفل الوسط.', 'Banners at the top end: hide after 5 s and stay under the pointer; "Important" stays until closed. The app toast sits at the bottom centre.')) + '</div>')
out('Ultar OS – الإشعارات المنبثقة', W, H, files_window(96, 40) + terminal_window(24, 112) + desk_icons(6) + stack + toast + legend + bottombar(running=(('files', 1), ('terminal', 1)), active='files'),
    'Hülle · Benachrichtigungs-Banner (normal, mit Aktion, wichtig) und Toast mit Rückgängig (§5.3)', 6)

# ================= compact (phone) =================
def statusbar(lock=False):
    """Status bar of shell-mobile (§5.5): time at the start, state icons at the end; the icon group opens the quick settings
    (as the status button of the bar); a swipe down anywhere on it does the same."""
    icons = ''.join(ic(n, True) for n in ('wifi', 'cell', 'battery')) + f'<span>{ltr("87%")}</span>'
    end = (f'<span aria-hidden="true" style="display: inline-flex; align-items: center; gap: {s2}">{icons}</span>' if lock else
           f'<button{act("mobile.status", S()["quick"])} aria-label="{S()["quick"]}" aria-haspopup="dialog" aria-expanded="false" style="height: {s6}; padding: 0 {s2}; border-radius: {r_ctl}; display: inline-flex; align-items: center; gap: {s2}">{icons}</button>')
    time = '<span></span>' if lock else f'<span style="font-weight: {w6}">{ltr("16:52")}</span>'
    return f'<div style="position: absolute; inset-inline: 0; top: 0; height: {s6}; padding: 0 {s3}; box-sizing: border-box; display: flex; align-items: center; justify-content: space-between; font-size: {f12}; z-index: 4">{time}{end}</div>'
def gesturebar():
    """Gesture bar: tap or swipe up = home, swipe up and hold = recent apps, swipe in from the start edge = back."""
    lab = T('mobile.home', 'الشاشة الرئيسية', 'Home')
    return f'<button{act("mobile.home", lab)} aria-label="{lab}" style="position: absolute; inset-inline: 0; bottom: 0; height: {touch}; z-index: 4; display: flex; align-items: center; justify-content: center"><span style="width: {g(28)}; height: {s1}; border-radius: {r_round}; background: {t.mute}"></span></button>'
def screen(body, top=True, bottom=True):
    return f'<main style="position: absolute; inset-inline: 0; top: {s6 if top else "0"}; bottom: {touch if bottom else "0"}; padding: {s3} {s4}; box-sizing: border-box; display: flex; flex-direction: column; gap: {s4}; overflow: hidden">{body}</main>'
def ptile_grid(apps, cols=4):
    return f'<div style="display: grid; grid-template-columns: repeat({cols}, minmax(0, 1fr)); gap: {s2}">' + ''.join(app_tile(i, n, 'mobile.app') for i, n, _ in apps) + '</div>'
def apptitle(title, back_action=None, end=''):
    b = ibtn(T('action.back', 'رجوع', 'Back'), 'back', back_action, small=False, big=True) if back_action else ''
    return f'<div style="display: flex; align-items: center; gap: {s2}; min-height: {touch}">{b}<div style="flex: 1; min-width: 0">{h1(title) if title else ""}</div>{end}</div>'

# --- home ---
begin('CompactHome.dc.html')
def home_body():
    clock = f'<div style="text-align: center; padding-top: {s5}"><div style="font-family: {FF_TITLE}; font-size: {f32}; font-weight: {w7}">{ltr("16:52")}</div><div style="font-size: {f16}">{tx("الأحد 22 تشرين الثاني")}</div>{sub("21 جمادى الآخرة 1448")}</div>'
    srch = sbox(T('mobile.search', 'ابحث في الهاتف', 'Search the phone'), 'mobile.search')
    home = [a for a in APPS9 if a[0] in ('folder', 'gear', 'editor', 'spark')]
    favs = [a for a in APPS9 if a[0] in ('phone', 'browser', 'bag')]
    allb = app_tile('grid', S()['apps'], 'mobile.apps')
    fav = f'<nav aria-label="{T("mobile.favorites", "المفضّلة", "Favourites")}" style="padding: {s2}; border-radius: {r_card}; background: {t.surface}; border: {HAIR} solid {t.line}; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: {s2}">' + ''.join(app_tile(i, n, 'mobile.app') for i, n, _ in favs) + allb + '</nav>'
    hint_ = f'<div style="text-align: center">{sub(T("mobile.home.hint", "اسحب للأعلى لكل التطبيقات", "Swipe up for all apps"))}</div>'
    return clock + srch + ptile_grid(home) + '<div style="flex: 1"></div>' + hint_ + fav
out('Ultar OS – الهاتف: الشاشة الرئيسية', PW, PH, statusbar() + screen(home_body()) + gesturebar(), 'Kompakt 360×800 · Startbildschirm: Statusleiste, Suche, Favoritenzeile, Gestenleiste (§5.5)', 7)

# --- app grid ---
begin('CompactApps.dc.html')
apps_body = (sbox(T('shell.apps.search', 'ابحث في التطبيقات', 'Search apps'), 'apps.search')
             + f'<div style="overflow: hidden">{cat_tabs()}</div>'
             + f'<section aria-label="{S()["apps"]}" style="display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: {s2}">' + ''.join(app_tile(i, n) for i, n, _ in ALPHA) + '</section>'
             + '<div style="flex: 1"></div>' + note(T('mobile.apps.note', 'اضغط مطوّلاً على تطبيق لقائمته: فتح، إلى المفضّلة، التفاصيل في المتجر، إزالة.', 'Press and hold an app for its menu: Open, Add to favourites, Details in the Store, Remove.')))
out('Ultar OS – الهاتف: كل التطبيقات', PW, PH, statusbar() + screen(apps_body) + gesturebar(), 'Kompakt · Alle Apps: Suche, Kategorien (Reiter rollen), Raster 4 Spalten (§5.1/§5.5)', 7)

# --- quick settings (swipe down from the top) ---
begin('CompactQuick.dc.html')
tiles = (f'<div style="display: grid; grid-template-columns: 1fr 1fr; gap: {s2}">' + qt('wifi', 'واي فاي', 'KM-Office', True, 'qs.wifi', 'qs.wifi.list') + qt('cell', T('qs.mobile', 'البيانات', 'Mobile data'), 'SIM 1', True, 'qs.mobile', 'qs.mobile.list')
         + qt('bluetooth', 'بلوتوث', OFF, False, 'qs.bluetooth', 'qs.bluetooth.list') + qt('plane', 'وضع الطيران', MOFF, False, 'qs.toggle')
         + qt('moon', 'الضوء الليلي', 'حتى 06:10', True, 'qs.toggle') + qt('bell-off', 'عدم الإزعاج', MOFF, False, 'qs.toggle') + '</div>')
sliders = slider('السطوع', 78, 'qs.brightness', icon='sun') + f'<div style="display: flex; align-items: center; gap: {s2}">{slider("مستوى الصوت", 62, "qs.volume", icon="volume")}{ibtn("جهاز إخراج الصوت: مكبّر الهاتف", "speaker", "qs.output")}</div>'
qhead = f'<div style="display: flex; align-items: center; gap: {s2}"><div style="flex: 1"><div style="font-family: {FF_TITLE}; font-size: {f20}; font-weight: {w7}">{ltr("16:52")}</div>{sub("الأحد 22 تشرين الثاني")}</div>{button(tx("87%"), "qs.battery", "quiet", icon="battery", name="البطارية 87%")}{ibtn("الإعدادات", "gear", "qs.settings")}{ibtn("خيارات الطاقة", "power", "qs.power")}</div>'
notes = (f'<div style="display: flex; align-items: center; gap: {s2}">{h2("الإشعارات", f14)}<span style="flex: 1"></span>{button("مسح الكل", "note.clear", "quiet")}</div>'
         + f'<ul role="list" style="display: flex; flex-direction: column; gap: {s2}">' + notif('phone', 'الهاتف', 'مكالمة فائتة من ليلى حداد', 'عبر رقم Ultar', '16:20', button('معاودة الاتصال', 'note.action', name='ليلى حداد: معاودة الاتصال')) + '</ul>')
handle_lab = T('mobile.quick.close', 'إغلاق الإعدادات السريعة', 'Close quick settings')
handle = f'<button{act("sheet.close", handle_lab)} aria-label="{handle_lab}" style="height: {tgt}; display: flex; align-items: center; justify-content: center"><span style="width: {g(12)}; height: {s1}; border-radius: {r_round}; background: {t.mute}"></span></button>'
qsheet = f'<section role="dialog" aria-modal="true" aria-label="{S()["quick"]}" style="position: absolute; inset-inline: 0; top: 0; z-index: 5; border-radius: 0 0 {r_card} {r_card}; background: {t.surface}; border-bottom: {HAIR} solid {t.line}; box-shadow: {t.shadow}; padding: {s4} {s4} 0; box-sizing: border-box; display: flex; flex-direction: column; gap: {s3}">{qhead}{tiles}{sliders}{notes}{handle}</section>'
behind = f'<div aria-hidden="true" style="position: absolute; inset: 0; z-index: 3; background: {t.scrim}"></div>'
out('Ultar OS – الهاتف: الإعدادات السريعة', PW, PH, statusbar() + behind + qsheet + gesturebar(), 'Kompakt · Schnelleinstellungen + Benachrichtigungen (Wisch von oben, §5.1/§5.5)', 7)

# --- lock screen ---
begin('CompactLock.dc.html')
def ln(icon, app, text):
    return f'<li style="border-radius: {r_card}; background: {t.surface}; border: {HAIR} solid {t.line}; padding: {s3}; display: flex; align-items: center; gap: {s3}"><span style="display: inline-flex; color: {t.mute}">{ic(icon)}</span><div style="flex: 1; min-width: 0"><div style="font-weight: {w6}">{tx(app)}</div>{sub(text)}</div></li>'
lclock = f'<div style="text-align: center; padding-top: {s6}"><div role="timer" style="font-family: {FF_TITLE}; font-size: {f32}; font-weight: {w7}">{ltr("16:52")}</div><div style="font-size: {f16}">{tx("الأحد 22 تشرين الثاني 2026")}</div>{sub("21 جمادى الآخرة 1448")}</div>'
lnotes = f'<ul role="list" aria-label="الإشعارات" style="display: flex; flex-direction: column; gap: {s2}">{ln("phone", "الهاتف", "مكالمة فائتة واحدة · المحتوى مخفي")}{ln("bag", "متجر Ultar", "إشعار واحد · المحتوى مخفي")}</ul>'
lmedia = f'<section aria-label="التحكّم بالوسائط" style="border-radius: {r_card}; background: {t.surface}; border: {HAIR} solid {t.line}; padding: {s3}; display: flex; flex-direction: column; gap: {s2}"><div><div style="font-weight: {w6}">يا مال الشام</div>{sub("صباح فخري")}</div><div style="display: flex; justify-content: center; gap: {s4}">{ibtn("المقطع السابق", "prev", "lock.media", small=False, big=True)}{ibtn("تشغيل", "play", "lock.media", small=False, big=True)}{ibtn("المقطع التالي", "next", "lock.media", small=False, big=True)}</div></section>'
lunlock = f'<div style="display: flex; flex-direction: column; align-items: center; gap: {s2}">{button("فتح القفل", "lock.unlock", "primary", icon="lock")}{sub(T("mobile.lock.swipe", "أو اسحب للأعلى من أسفل الشاشة", "or swipe up from the bottom edge"))}</div>'
lbottom = f'<div style="display: flex; justify-content: space-between; gap: {s2}">{button("طوارئ", "lock.sos", icon="sos")}{button("إيقاف التشغيل الطارئ", "lock.power", icon="power")}</div>'
out('Ultar OS – الهاتف: شاشة القفل', PW, PH, statusbar(lock=True) + screen(lclock + lnotes + lmedia + '<div style="flex: 1"></div>' + lunlock + lbottom, bottom=False), 'Kompakt · Sperrbildschirm (§5.3)', 7)

# --- settings: list -> page (§8: compact = list, then page) ---
begin('CompactSettings.dc.html')
def arow(icon, label, subtext, last=False):
    return (f'<button{act("settings.area", label)} style="width: 100%; min-height: {ROW_MIN}; padding: {s2} {s4}; box-sizing: border-box; display: flex; align-items: center; gap: {s4}; border-bottom: {"none" if last else HAIR + " solid " + t.line}">'
            f'<span style="display: inline-flex; color: {t.mute}">{ic(icon)}</span><span style="flex: 1; min-width: 0; display: block"><span style="display: block; font-weight: {w5}">{tx(label)}</span>{"<span style=\"display: block; font-size: " + f12 + "; color: " + t.mute + "\">" + tx(subtext) + "</span>" if subtext else ""}</span><span style="display: inline-flex; color: {t.mute}">{ic("forward", True)}</span></button>')
AREAS_C = [('wifi', 'واي فاي', 'KM-Office'), ('cell', T('settings.mobile_network', 'الشبكة الخلوية', 'Mobile network'), 'SIM 1'), ('bluetooth', 'بلوتوث', OFF), ('monitor', 'الشاشات', T('settings.display.summary', 'السطوع، الضوء الليلي', 'Brightness, night light')),
           ('volume', 'الصوت', ''), ('battery', 'الطاقة', '87%'), ('bell', 'الإشعارات', ''), ('palette', 'المظهر', 'داكن'), ('apps', 'التطبيقات', ''), ('shield', 'الخصوصية والأمان', '')]
slist = card(''.join(arow(i, l, s, n == len(AREAS_C) - 1) for n, (i, l, s) in enumerate(AREAS_C)), label=T('settings.areas', 'أقسام الإعدادات', 'Settings areas'))
out('Ultar OS – الهاتف: الإعدادات', PW, PH, statusbar() + screen(apptitle('الإعدادات') + sbox('ابحث في الإعدادات', 'settings.search') + slist) + gesturebar(), 'Kompakt · Einstellungen: Liste der Bereiche (§8: Liste → Seite)', 7)

begin('CompactDisplay.dc.html')
bright = f'<div style="padding: {s2} {s4} {s3}; border-bottom: {HAIR} solid {t.line}; display: flex; flex-direction: column; gap: {s1}"><div style="font-weight: {w5}">السطوع</div>{slider("السطوع", 78, "display.brightness", icon="sun")}</div>'
nl = f'<div style="display: flex; align-items: center; gap: {s2}">{switch(True, "الضوء الليلي", "display.nightlight")}{ibtn("الضوء الليلي: الجدول ودرجة الدفء", "forward", "display.nightlight.more")}</div>'
dcard = card(f'<div style="padding: {s4}; border-bottom: {HAIR} solid {t.line}">{h2(T("settings.display.builtin", "الشاشة المدمجة", "Built-in display"))}{sub(ltr("1080 × 2340 · 6.4″"))}</div>' + bright
             + row(T('settings.display.auto_brightness', 'السطوع التلقائي', 'Automatic brightness'), T('settings.display.auto_brightness.sub', 'يتكيّف مع الضوء من حولك', 'Follows the light around you'), switch(True, 'السطوع التلقائي', 'display.autobrightness'))
             + row('الضوء الليلي', 'حتى 06:10', nl)
             + row('حجم العناصر', '', select('300% (موصى به)', 'حجم العناصر', 'display.scale'))
             + row('معدّل التحديث', '', select('120 Hz', 'معدّل التحديث', 'display.refresh'))
             + row('الاتجاه', '', select(T('settings.display.orientation.auto', 'تلقائي', 'Automatic'), 'الاتجاه', 'display.orientation'))
             + row('HDR', 'غير مدعوم على هذه الشاشة', switch(False, 'HDR', 'display.hdr', 'disabled'), True))
dnote = note(T('settings.display.compact_note', 'الترتيب ووضع العرض يظهران عند توصيل شاشة خارجية.', 'Arrangement and display mode appear when an external display is connected.'))
out('Ultar OS – الهاتف: الشاشات', PW, PH, statusbar() + screen(apptitle('الشاشات', 'settings.back') + dcard + dnote) + gesturebar(), 'Kompakt · Einstellungen › Anzeige (Seite mit Zurück)', 7)

# --- Store: list and app page (§7.3) ---
STORE_C = [('office', 'الأدوات المكتبية', True), ('code', 'المبرمج', True), ('media', 'الوسائط', False), ('mail', 'التواصل', False), ('spark', 'المساعد والذكاء الاصطناعي المحلي', True), ('firefox', 'Firefox', True)]
verified = T('store.verified', 'KM Code Craft · موثَّق', 'KM Code Craft · verified')
begin('CompactStore.dc.html')
def srow(icon, name, installed, last=False):
    state = badge('مثبَّت', 'ok', 'check') if installed else badge('غير مثبَّت')
    return (f'<li style="border-bottom: {"none" if last else HAIR + " solid " + t.line}"><button{act("store.open", name)} aria-label="{plain(name)}: {T("store.details", "التفاصيل", "details")}" style="width: 100%; min-height: {ROW_MIN}; padding: {s2} {s3}; box-sizing: border-box; display: flex; align-items: center; gap: {s3}">'
            f'<span style="width: {touch}; height: {touch}; flex: none; border-radius: {r_ctl}; background: {t.raised}; border: {HAIR} solid {t.line}; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center">{ic(icon)}</span>'
            f'<span style="flex: 1; min-width: 0; display: block"><span style="display: block; font-weight: {w6}">{tx(name)}</span><span style="font-size: {f12}; color: {t.mute}; display: flex; align-items: center; gap: {s1}">{ic("check", True)}{tx(verified)}</span></span>{state}</button></li>')
more = ibtn(T('store.more_areas', 'أقسام أخرى: تثبيت من ملف، مصادر بلا إنترنت، الحساب', 'More areas: install from a file, offline sources, account'), 'more', 'store.more', small=False, big=True)
stabs = tablist(tab('استكشاف', True, a='store.area') + tab('المثبَّتة', a='store.area') + tab('التحديثات', a='store.area'), 'أقسام المتجر')
slist2 = card('<ul role="list">' + ''.join(srow(i, n, inst, k == len(STORE_C) - 1) for k, (i, n, inst) in enumerate(STORE_C)) + '</ul>', label='مكوّنات من Ultar')
out('Ultar Store – الهاتف: استكشاف', PW, PH, statusbar() + screen(apptitle('متجر Ultar', end=more) + sbox('ابحث عن تطبيق أو نوع ملف', 'store.search') + stabs + f'<div>{h2("مكوّنات من Ultar", f14)}</div>' + slist2) + gesturebar(), 'Kompakt · Ultar Store: Entdecken als Liste, Bereiche als Reiter + Menü (§7.3)', 7)

begin('CompactStoreApp.dc.html')
head = (f'<div style="display: flex; align-items: center; gap: {s3}"><span style="width: {s7}; height: {s7}; flex: none; border-radius: {r_ctl}; background: {t.raised}; border: {HAIR} solid {t.line}; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center">{ic("office")}</span>'
        f'<div style="flex: 1; min-width: 0"><div style="font-family: {FF_TITLE}; font-size: {f20}; font-weight: {w7}">{tx("الأدوات المكتبية")}</div><div style="font-size: {f12}; color: {t.mute}; display: flex; align-items: center; gap: {s1}">{ic("check", True)}{tx(verified)}</div></div></div>')
inst = f'<div style="display: flex">{button(T("store.install", "تثبيت", "Install"), "store.install", "primary", grow=True)}</div>'
def fact(label, value):
    return f'<div style="flex: 1; text-align: center">{sub(label)}<div style="font-weight: {w6}">{tx(value)}</div></div>'
facts = f'<div style="display: flex; padding: {s3} 0; border-top: {HAIR} solid {t.line}; border-bottom: {HAIR} solid {t.line}">{fact(T("store.size", "الحجم", "Size"), NA)}{fact(T("store.version", "الإصدار", "Version"), NA)}{fact(T("store.source", "المصدر", "Source"), "Ultar")}</div>'
desc = f'<div style="color: {t.mute}">{tx(T("store.office.desc", "مستندات وجداول وعروض وأدوات PDF، تعمل دون إنترنت وتفتح ملفات .docx و .xlsx", "Documents, spreadsheets, slides and PDF tools; they work offline and open .docx and .xlsx files."))}</div>'
rights = card(cardhead(T('store.rights', 'الصلاحيات بلغة واضحة', 'Rights in plain words'))
              + row(T('store.rights.files', 'الملفات', 'Files'), T('store.rights.files.office', 'يفتح فقط الملفات التي تختارها أنت', 'Opens only the files you choose'), '')
              + row(T('store.rights.network', 'الشبكة', 'Network'), T('store.rights.network.none', 'لا يتّصل بالإنترنت', 'Does not connect to the internet'), '')
              + row(T('store.rights.print', 'الطباعة', 'Printing'), T('store.rights.print.office', 'يطبع على الطابعة التي تختارها', 'Prints on the printer you choose'), '', True))
snote = note('— = الحجم والإصدار من الكتالوج عند التشغيل؛ لا قيمة لهما في المواصفة.')
out('Ultar Store – الهاتف: صفحة التطبيق', PW, PH, statusbar() + screen(apptitle('', 'store.back') + head + inst + facts + desc + rights + snote) + gesturebar(), 'Kompakt · Ultar Store: App-Seite (Rechte in Klartext, Installieren)', 7)
