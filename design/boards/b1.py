# Desktop: Main (dark, Arabic, standard), DesktopBottom (light, English, standard), DesktopTop (variant).
from gen import *

def place(icon, label, active=False, a='files.place', end=''):
    return f'<div style="display: flex; align-items: center; gap: {s1}">{navitem(icon, label, a, active)}{end}</div>'
def item(icon, label, a='files.item', selected=False):
    return f'<button{act(a, label)} aria-label="{plain(label)}" style="width: {g(26)}; padding: {s2} {s1}; box-sizing: border-box; border-radius: {r_ctl}; display: flex; flex-direction: column; align-items: center; gap: {s1}; font-size: {f12}; background: {t.hover if selected else "transparent"}; border: {HAIR} solid {t.ctl if selected else "transparent"}"><span style="width: {s7}; height: {s7}; border-radius: {r_ctl}; background: {t.raised}; border: {HAIR} solid {t.line}; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center">{ic(icon)}</span><bdi>{label}</bdi></button>'
def files_window(x, y):
    side = sidebar(L('الأماكن', 'Places'), place('home', 'المجلد الرئيسي') + place('place-documents', 'المستندات', True) + place('place-downloads', 'التنزيلات') + place('place-pictures', 'الصور') + place('place-music', 'الموسيقى')
        + f'<div aria-hidden="true" style="height: {HAIR}; background: {t.line}; margin: {s1} 0"></div>'
        + place('usb', 'ذاكرة USB', a='files.drive', end=ibtn('إخراج ذاكرة USB', 'eject', 'files.eject')) + place('bin', 'سلة المحذوفات', a='files.trashplace'), 52)
    tools = f'<div style="height: {s7}; flex: none; display: flex; align-items: center; gap: {s2}; padding: 0 {s3}; border-bottom: {HAIR} solid {t.line}">{h2("المستندات", f14)}<span style="color: {t.mute}; font-size: {f12}">{tx("6 عناصر")}</span><span style="flex: 1"></span>{searchbox("ابحث في المستندات", "files.search", width=g(50))}{ibtn("مجلد جديد", "plus", "files.newfolder")}</div>'
    grid = f'<div style="padding: {s3}; display: flex; flex-wrap: wrap; gap: {s2}; align-content: flex-start">{item("kind-folder", "مشاريع", selected=True)}{item("kind-folder", "عقود")}{item("kind-folder", "فواتير")}{item("kind-text", "تقرير.docx")}{item("kind-sheet", "ميزانية.xlsx")}{item("kind-image", "مخطط.png")}</div>'
    body = f'<div style="flex: 1; display: flex; min-height: 0">{side}<div style="flex: 1; display: flex; flex-direction: column; min-width: 0">{tools}{grid}</div></div>'
    nav = ibtn('رجوع', 'back', 'files.back') + ibtn('إلى الأمام', 'forward', 'files.forward', state='disabled')
    return window(x, y, 208, 124, 'الملفات', body, nav, z=2)
def terminal_window(x, y):
    term = f'<div style="flex: 1; padding: {s3} {s4}; font-family: {FF_MONO}; color: {t.text}; direction: ltr; text-align: left; background: {t.ground}"><div><span style="color: {t.ok}">anas@ultar</span>:~$ uapp list</div><div style="color: {t.mute}">files · browser · terminal · settings · store</div><div style="display: flex; align-items: center; gap: {s1}"><span><span style="color: {t.ok}">anas@ultar</span>:~$</span><span aria-hidden="true" style="width: {s2}; height: {ic_s}; background: {t.text}"></span></div></div>'
    return window(x, y, 132, 64, 'الطرفية', term, z=1)
def desk_icons(top):
    return f'<div style="position: absolute; top: {g(top)}; inset-inline-start: {s5}; display: flex; flex-direction: column; gap: {s2}">{item("home", "المجلد الرئيسي", "desk.icon")}{item("usb", "ذاكرة USB", "desk.icon")}</div>'

# focus order: active window, other windows, desktop icons, bar (A34)
begin('Main.dc.html')
out('Ultar OS – سطح المكتب', 360, 225, files_window(88, 18) + terminal_window(40, 132) + desk_icons(6) + bottombar(running=(('files', 1), ('terminal', 1)), active='files'),
    'Desktop · dunkel · Arabisch (Standard: eine Leiste unten)', 1)

begin('DesktopBottom.dc.html', 'light', False)
def tab(label, active):
    return f'<div style="height: {tgt}; display: flex; align-items: center; border-radius: {r_ctl}; background: {t.surface if active else "transparent"}; border: {HAIR} solid {t.ctl if active else "transparent"}; box-sizing: border-box"><button role="tab" aria-selected="{"true" if active else "false"}"{act("browser.tab", label)} style="height: 100%; padding: 0 {s3}; font-weight: {w6 if active else w4}">{label}</button>{ibtn("Close tab " + label, "close", "browser.tabclose")}</div>'
tabs = f'<div role="tablist" aria-label="Tabs" style="height: {s7}; flex: none; display: flex; align-items: center; gap: {s1}; padding: 0 {s2}; background: {t.raised}; border-bottom: {HAIR} solid {t.line}">{tab("New tab", True)}{tab("code-craft.tech", False)}{ibtn("New tab", "plus", "browser.newtab")}</div>'
protect = f'<button{act("browser.shield", "Tracking protection: on")} aria-label="Tracking protection: on" aria-haspopup="dialog" style="height: {tgt}; padding: 0 {s2}; border-radius: {r_ctl}; display: inline-flex; align-items: center; gap: {s1}; color: {t.ok}">{ic("block", True)}{ic("check", True)}</button>'
addr = f'<div style="height: {s7}; flex: none; display: flex; align-items: center; gap: {s2}; padding: 0 {s3}; border-bottom: {HAIR} solid {t.line}">{ibtn("Back", "back", "browser.back", state="disabled")}{ibtn("Forward", "forward", "browser.forward", state="disabled")}{ibtn("Reload", "reload", "browser.reload")}{field("Search or enter address", "browser.address", hint="Search or enter address", icon="search", show_label=False)}{protect}{ibtn("Downloads", "download", "browser.downloads")}</div>'
start = f'<div style="flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: {s5}"><div style="display: flex; align-items: center; gap: {s3}">{mark()}<span style="font-family: {FF_TITLE}; font-size: {f24}; font-weight: {w7}">Ultar Browser</span></div>{field("Search the web privately", "browser.address", hint="Search the web privately", icon="search", show_label=False, width=g(120))}<div style="font-size: {f12}; color: {t.mute}; display: flex; align-items: center; gap: {s1}">{ic("block", True)}<span>Ads and trackers are blocked on every page.</span></div></div>'
out('Ultar OS – desktop, single bar', 360, 225, window(56, 16, 248, 156, 'Ultar Browser', tabs + addr + start, z=2) + bottombar(running=(('browser', 1),), active='browser', usb=False),
    'Desktop · hell · Englisch (Standard: eine Leiste unten)', 1)

begin('DesktopTop.dc.html')
out('Ultar OS – سطح المكتب (الشريط في الأعلى)', 360, 225, files_window(88, 22) + terminal_window(40, 132) + desk_icons(12) + topbar('الملفات') + dock(running=(('files', 1), ('terminal', 1))),
    'Variante (umstellbar): Leiste oben + Dock', 1)
