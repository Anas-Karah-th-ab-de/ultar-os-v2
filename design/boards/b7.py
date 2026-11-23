# Popup and navigation bricks as ultar-ui draws them: Menus (dark, Arabic: menu, context menu with submenu,
# select list, tooltip) and Bricks (light, English: tabs, scrollbar, bottom sheet on a phone, toast).
from gen import *
W, H = 320, 200

def panel(inner, label, role='menu', width=48):
    """Menu, context menu and select list: raised panel, line hairline, control radius, the one shadow, padding 4."""
    return f'<div role="{role}" aria-label="{plain(label)}" style="width: {g(width)}; padding: {s1}; box-sizing: border-box; border-radius: {r_ctl}; background: {t.raised}; border: {HAIR} solid {t.line}; box-shadow: {t.shadow}; display: flex; flex-direction: column">{inner}</div>'
def mitem(label, icon=None, shortcut='', sub=False, danger=False, disabled=False, lit=False, checked=False, slot=True, chevrons=False):
    """One command row: height = pointer target, padding 8, slot 16 + 8 for icon or check, shortcut muted at the end,
    a forward chevron (mirrored with the direction) for a submenu; danger in its colour, disabled as every disabled brick (state token)."""
    color = t.danger if danger else t.text
    lead = ic('check', True) if checked else (ic(icon, True) if icon else '')
    lead = f'<span style="width: {ic_s}; height: {ic_s}; flex: none; display: inline-flex">{lead}</span>' if slot else ''
    end = f'<span style="font-size: {f12}; color: {t.mute}">{tx(shortcut)}</span>' if shortcut else ''
    tail = f'<span style="width: {ic_s}; height: {ic_s}; flex: none; display: inline-flex; color: {t.mute}">{ic("forward", True) if sub else ""}</span>' if chevrons else ''
    extra = (' aria-haspopup="menu" aria-expanded="%s"' % ('true' if lit else 'false')) if sub else ''
    state = (' aria-disabled="true"' if disabled else '') + (' aria-checked="true"' if checked else '')
    role = 'menuitemcheckbox' if checked else 'menuitem'
    return f'<button role="{role}"{act("menu.item", label)}{extra}{state} style="height: {tgt}; flex: none; padding: 0 {s2}; box-sizing: border-box; border-radius: {r_ctl}; display: flex; align-items: center; gap: {s2}; background: {t.hover if lit else "transparent"}; color: {color}{"; " + DIM if disabled else ""}">{lead}<span style="flex: 1">{tx(label)}</span>{end}{tail}</button>'
def msep():
    """Separator: 8 high with the hairline in its middle."""
    return f'<div role="separator" style="height: {s2}; flex: none; display: flex; align-items: center"><span style="height: {HAIR}; flex: 1; background: {t.line}"></span></div>'
def option(label, chosen=False, lit=False):
    """Option of an open select (as the toolkit draws it): chosen = text 600 with a check in accent_text at the end, highlighted = hover background."""
    mark = ic('check', True, t.acctext) if chosen else ''
    return f'<button role="option" aria-selected="{"true" if chosen else "false"}"{act("select.option", label)} style="height: {tgt}; flex: none; padding: 0 {s2}; box-sizing: border-box; border-radius: {r_ctl}; display: flex; align-items: center; gap: {s2}; background: {t.hover if lit else "transparent"}; font-weight: {w6 if chosen else w4}"><span style="flex: 1">{tx(label)}</span><span style="width: {ic_s}; height: {ic_s}; flex: none; display: inline-flex">{mark}</span></button>'
def tooltip(label):
    """Tooltip: raised, control_border hairline, padding 4/8, small text, the one shadow. Appears after motion.tooltip_delay."""
    return f'<div role="tooltip" style="padding: {s1} {s2}; border-radius: {r_ctl}; background: {t.raised}; border: {HAIR} solid {t.ctl}; box-shadow: {t.shadow}; font-size: {f12}">{tx(label)}</div>'
def caption(s):
    return f'<div style="font-size: {f12}; color: {t.mute}; font-weight: {w5}">{tx(s)}</div>'
def spot(cap, inner, x, y):
    return f'<div style="position: absolute; inset-inline-start: {g(x)}; top: {g(y)}; display: flex; flex-direction: column; gap: {s2}">{caption(cap)}{inner}</div>'

begin('Menus.dc.html')
menu = panel(mitem('جديد', 'plus', 'Ctrl+N') + mitem('فتح…', 'folder', 'Ctrl+O', lit=True) + mitem('حفظ', 'download', 'Ctrl+S', disabled=True) + msep()
             + mitem('حذف', 'trash', 'Del', danger=True), 'ملف', width=56)
opener = f'<button{act("menu.open", "ملف")} aria-haspopup="menu" aria-expanded="true" style="height: {tgt}; padding: 0 {s3}; border-radius: {r_ctl}; background: {t.line}; font-weight: {w6}; align-self: flex-start">{tx("ملف")}</button>'
ctx_main = panel(mitem('نسخ', 'copy', 'Ctrl+C', chevrons=True) + mitem('ترتيب حسب', sub=True, lit=True, chevrons=True) + msep() + mitem('عرض المخفيّة', checked=True, chevrons=True), 'قائمة السياق', width=56)
ctx_sub = panel(mitem('الاسم', checked=True) + mitem('التاريخ', slot=True) + mitem('الحجم', slot=True), 'ترتيب حسب', width=40)
ctx = f'<div style="display: flex; align-items: flex-start; gap: {s1}">{ctx_main}<div style="margin-top: {g(8)}">{ctx_sub}</div></div>'
sel = panel(option('60 Hz') + option('120 Hz', chosen=True) + option('144 Hz', lit=True), 'معدّل التحديث', 'listbox', 40)
seltrigger = select('120 Hz', 'معدّل التحديث', 'menu.open')
tip = f'<div style="display: flex; flex-direction: column; align-items: flex-start; gap: {s2}">{ibtn("الإعدادات", "gear", "menu.open", state="hover")}{tooltip("الإعدادات")}</div>'
legend = note('اللوحات من ultar-ui: سطر بارتفاع هدف المؤشّر، حشوة 8، خانة 16 للأيقونة أو العلامة، الاختصار باهت في النهاية. على الهاتف تُمرَّر القائمة الطويلة داخل اللوحة، والضغط الطويل يفتح قائمة السياق.')
body = (spot('قائمة', f'<div style="display: flex; flex-direction: column; gap: {s1}">{opener}{menu}</div>', 8, 8) + spot('قائمة السياق مع قائمة فرعية', ctx, 88, 8)
        + spot('قائمة اختيار مفتوحة', f'<div style="display: flex; flex-direction: column; gap: {s1}">{seltrigger}{sel}</div>', 196, 8) + spot('تلميح', tip, 264, 8)
        + f'<div style="position: absolute; inset-inline: {g(8)}; bottom: {g(8)}">{legend}</div>')
out('Ultar OS – القوائم والتلميح', W, H, f'<main style="position: absolute; inset: 0">{body}</main>', 'Bausteine: Menü, Kontextmenü mit Untermenü, Auswahlliste, Tooltip', 4)

begin('Bricks.dc.html', 'light', False)
def tab(label, chosen=False, state=''):
    """Underline tab: target height, padding 16, chosen = text 600 with a 2-point accent_text bar inset 8."""
    bg = {'hover': t.hover, 'pressed': t.line}.get(state, 'transparent')
    bar = f'<span style="position: absolute; inset-inline: {s2}; bottom: 0; height: {RING}; border-radius: {r_round}; background: {t.acctext}"></span>' if chosen else ''
    return f'<button role="tab" aria-selected="{"true" if chosen else "false"}"{"" if chosen else " tabindex=\"-1\""}{act("tabs.tab", label)} style="position: relative; height: {tgt}; min-width: {tgt}; padding: 0 {s4}; border-radius: {r_ctl}; background: {bg}; color: {t.text if chosen else t.mute}; font-weight: {w6 if chosen else w4}{state_extra(state)}">{tx(label)}{bar}</button>'
def tablist(inner):
    return f'<div role="tablist" aria-label="Settings pages" style="display: flex; border-bottom: {HAIR} solid {t.line}">{inner}</div>'
tabs = (tablist(tab('General', True) + tab('Display') + tab('Sound', state='hover') + tab('Network'))
        + tablist(tab('General') + tab('Display', True, 'focus') + tab('Sound', state='pressed') + tab('Network', state='disabled')))
def scrollarea(active):
    rows = ''.join(f'<div style="min-height: {ROW_MIN}; padding: 0 {s4}; display: flex; align-items: center; border-bottom: {HAIR} solid {t.line}">{tx(n)}</div>' for n in ('Documents', 'Downloads', 'Music', 'Pictures', 'Videos'))
    thumb = f'<span aria-hidden="true" style="position: absolute; inset-inline-end: {s1}; top: {g(4)}; width: {s1}; height: {g(16)}; border-radius: {r_round}; background: {t.mute if active else t.ctl}"></span>'
    return f'<div style="position: relative; width: {g(56)}; height: {g(48)}; overflow: hidden; border-radius: {r_card}; background: {t.surface}; border: {HAIR} solid {t.line}">{rows}{thumb}</div>'
scroll = f'<div style="display: flex; gap: {s4}">{scrollarea(False)}{scrollarea(True)}</div>'
def phone_sheet():
    """Bottom sheet on a phone: full width at the lower edge, card radius, raised, behind it the ground at state.scrim."""
    content = f'<div style="padding: {s4}; display: flex; flex-direction: column; gap: {s2}">{h2("Share")}{sub("Send the picture to a device nearby or copy the link.")}</div>'
    sheet_ = f'<section role="dialog" aria-modal="true" aria-label="Share" style="position: absolute; inset-inline: 0; bottom: 0; border-radius: {r_card} {r_card} 0 0; background: {t.raised}; border: {HAIR} solid {t.line}; box-shadow: {t.shadow}; display: flex; flex-direction: column; gap: {s2}; padding-bottom: {s4}">{content}<div style="padding: 0 {s4}; display: flex; flex-direction: column; gap: {s2}">{button("Copy link", "sheet.close", "primary", icon="copy")}{button("Close", "sheet.close")}</div></section>'
    page_ = f'<div style="padding: {s4}">{h2("Pictures")}{sub("12 items")}</div>'
    return f'<div style="position: relative; width: {g(90)}; height: {g(160)}; overflow: hidden; border-radius: {r_card}; background: {t.ground}; border: {HAIR} solid {t.line}">{page_}<div style="position: absolute; inset: 0; background: {t.scrim}"></div>{sheet_}</div>'
def toast():
    """Toast: size.toast_width wide, target + 2 x 8 high; raised, card radius, hairline in the tone colour, tone icon 24, action in accent_text 600, close button."""
    return (f'<div role="status" style="width: {TOAST_W}; height: {g(12)}; padding: 0 {s3}; box-sizing: border-box; border-radius: {r_card}; background: {t.raised}; border: {HAIR} solid {t.ok}; box-shadow: {t.shadow}; display: flex; align-items: center; gap: {s2}">'
            f'<span style="display: inline-flex; color: {t.ok}">{ic("check")}</span><span style="flex: 1">{tx("3 files moved to the Trash.")}</span>'
            f'<button{act("toast.action", "Undo")} style="height: {tgt}; padding: 0 {s2}; border-radius: {r_ctl}; color: {t.acctext}; font-weight: {w6}">{tx("Undo")}</button>{ibtn("Close", "close", "toast.close")}</div>')
legend = note('Tabs keep the target size and scroll when they do not fit. The scrollbar is 4 wide at 4 from the end edge (12 to grip), control_border at rest, text_muted while hovered or dragged.')
body = (spot('Tabs (normal, hovered, focused, pressed, disabled)', f'<div style="display: flex; flex-direction: column; gap: {s4}">{tabs}</div>', 8, 8)
        + spot('Scrollbar (at rest, active)', scroll, 8, 56) + spot('Bottom sheet (phone)', phone_sheet(), 140, 8) + spot('Toast', toast(), 8, 120)
        + f'<div style="position: absolute; inset-inline: {g(8)}; bottom: {g(8)}">{legend}</div>')
out('Ultar OS – navigation and sheet bricks', W, H, f'<main style="position: absolute; inset: 0">{body}</main>', 'Bausteine: Tabs, Scrollleiste, Bottom Sheet, Toast (hell, Englisch)', 4)
