# Generates the artboards of the Ultar OS desktop design canvas.
# EVERY visual value comes from design/tokens.toml. This file holds the only derivations (marked "derived").
import os, re, tomllib
from icons import I as ICONS
from actions import ACTIONS
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, 'project')
TOKENS = os.environ.get('ULTAR_TOKENS', '/home/claude/ultar-os-v2/design/tokens.toml')
TOK = tomllib.load(open(TOKENS, 'rb'))

def px(n):
    return '%dpx' % n
_sp = TOK['space']['values']
s1, s2, s3, s4, s5, s6, s7 = [px(v) for v in _sp]          # 4 8 12 16 24 32 48
BASE = _sp[0]
def g(n):
    """Layout length: n units of the base grid (derived: n x space.values[0])."""
    return px(n * BASE)
_rd = TOK['radius']
r_ctl, r_card, r_round = px(_rd['control']), px(_rd['card']), px(_rd['round'])
_ty = TOK['type']
f12, f14, f16, f20, f24, f32 = [px(v) for v in _ty['sizes']]
w4, w5, w6, w7 = _ty['weights']
_sz = TOK['size']
bar_h, title_h = px(_sz['bar_height']), px(_sz['titlebar_height'])
tgt, touch = px(_sz['target_pointer_min']), px(_sz['target_touch_min'])   # control height = pointer target
IC_S, IC_L = _sz['icon_small'], _sz['icon']
ic_s, ic_l = px(IC_S), px(IC_L)
STROKE = _sz['stroke']
RING = px(_sz['focus_ring'])
BORDER = _sz['border']
HAIR = px(BORDER)             # size.border: every hairline (cards, fields, check box, rules)
EDGE2 = px(2 * BORDER)        # derived: the edge of the primary button is two borders wide (3:1 next to the gold)
ROW_MIN = px(_sz['row_min'])  # minimum height of a list or settings row
BOX = px(_sz['box'])          # check box and radio circle
TOAST_W = px(_sz['toast_width'])  # a toast
_st = TOK['state']
_sh = TOK['shadow']
FF_TEXT = "'%s', '%s', sans-serif" % (_ty['family_latin'], _ty['family_arabic_text'])
FF_TITLE = "'%s', '%s', sans-serif" % (_ty['family_latin'], _ty['family_arabic_title'])
FF_MONO = "'%s', '%s', monospace" % (_ty['family_mono'], _ty['family_arabic_text'])
_wg = ';'.join(str(w) for w in _ty['weights'])
LINK = '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?%s&display=swap">' % '&'.join(
    'family=%s:wght@%s' % (_ty[k].replace(' ', '+'), _wg) for k in ('family_latin', 'family_arabic_text', 'family_arabic_title', 'family_mono'))

def rgba(hexcolor, alpha):
    """A theme colour at a token alpha (scrim, state overlays)."""
    return 'rgba(%d,%d,%d,%.2f)' % (int(hexcolor[1:3], 16), int(hexcolor[3:5], 16), int(hexcolor[5:7], 16), alpha)
def over(base, fg, share):
    """A filled colour with its state layer: `fg` at `share` (state.hover_overlay / pressed_overlay) over `base`."""
    layer = rgba(fg, share)
    return 'linear-gradient(%s, %s), %s' % (layer, layer, base)
DIM = 'opacity: %.2f' % _st['disabled_opacity']   # state.disabled_opacity: the whole widget
class Theme:
    def __init__(self, name):
        d = TOK['theme'][name]; self.name = name
        self.ground, self.bar, self.surface, self.raised, self.hover, self.line = d['ground'], d['bar'], d['surface'], d['raised'], d['hover'], d['line']
        self.ctl, self.text, self.mute = d['control_border'], d['text'], d['text_muted']
        self.acc, self.onacc, self.acctext = d['accent'], d['on_accent'], d['accent_text']
        self.info, self.ok, self.warn, self.danger = d['info'], d['success'], d['warning'], d['danger']
        alpha = _sh['alpha_' + name]
        self.shadow = '0 %dpx %dpx rgba(0,0,0,%.2f)' % (_sh['offset_y'], _sh['blur'], alpha)
        self.scrim = rgba(self.ground, _st['scrim_' + name])   # state.scrim_*: ground laid over what lies behind a dialog
        self.focus = d[_st['focus_ring']]                       # state.focus_ring names the colour role of the ring
THEMES = {'dark': Theme('dark'), 'light': Theme('light')}
class _Cx:
    name = ''; rtl = True; lang = 'ar'; theme = THEMES['dark']; uid = 0
cx = _Cx()
class _T:
    def __getattr__(self, k):
        return getattr(cx.theme, k)
t = _T()
def lh():
    return str(_ty['line_height_arabic'] if cx.rtl else _ty['line_height_latin'])

# ---------- text ----------
_LAT = re.compile(r'[.+]?[A-Za-z0-9][A-Za-z0-9 .,:×%/+\-″@_]*[A-Za-z0-9%″]|[.+]?[A-Za-z0-9]')
def ltr(x):
    return '<bdi dir="ltr">%s</bdi>' % x
def tx(s):
    """Visible text. In Arabic every Latin word, number and unit becomes a left-to-right island."""
    s = str(s)
    if not cx.rtl or '<' in s:
        return s
    return _LAT.sub(lambda m: ltr(m.group(0)), s)
def plain(s):
    return re.sub(r'<[^>]+>', '', str(s)).replace('"', '').replace('‎', '').strip()
def L(ar, en):
    return ar if cx.rtl else en

# ---------- clicks ----------
CLICKS = {}; ICON_USE = {}
def act(a, name):
    assert a in ACTIONS, 'undefined action: ' + a
    CLICKS.setdefault(cx.name, []).append((plain(name), a))
    return ' data-act="%s"' % a
def uid(p='i'):
    cx.uid += 1
    return '%s%d' % (p, cx.uid)

# ---------- icons ----------
def ic(name, small=False, color='currentColor'):
    d = ICONS[name]; s = IC_S if small else IC_L
    ICON_USE.setdefault(name, set()).add(cx.name)
    flip = ' style="transform: scaleX(-1)"' if (d['mirror'] and cx.rtl) else ''
    return '<svg aria-hidden="true" data-icon="%s" width="%d" height="%d" viewBox="0 0 24 24" fill="none" stroke="%s" stroke-width="%s" stroke-linecap="round" stroke-linejoin="round"%s><path d="%s"></path></svg>' % (name, s, s, color, STROKE, flip, d['d'])
def mark(small=False):
    """Brand mark = Overview button (section 5.1)."""
    s = IC_S if small else IC_L
    return '<svg aria-hidden="true" data-icon="mark" width="%d" height="%d" viewBox="0 0 24 24" fill="none" stroke="%s" stroke-width="%s" stroke-linecap="round"><path d="M12 3a9 9 0 1 0 9 9"></path><path d="M12 8a4 4 0 1 0 4 4"></path></svg>' % (s, s, t.acctext, STROKE)
def picto(kind):
    """Two-display pictogram. Physical picture of two screens: never mirrored (decision R11), no arrow."""
    def scr(x, mode):
        a = {'on': 'fill="currentColor"', 'off': 'stroke-dasharray="%d %d"' % (_sz['focus_ring'], _sz['focus_ring']), 'line': ''}[mode]
        return '<rect x="%d" y="4" width="20" height="16" %s></rect>' % (x, a)
    a, b, extra = {'extend': ('line', 'line', '<path d="M16 12h16"></path>'), 'mirror': ('on', 'on', ''), 'first': ('on', 'off', ''), 'second': ('off', 'on', '')}[kind]
    return '<svg aria-hidden="true" data-illus="display-mode-%s" width="48" height="24" viewBox="0 0 48 24" fill="none" stroke="currentColor" stroke-width="%s">%s%s%s</svg>' % (kind, STROKE, scr(2, a), scr(26, b), extra)

# ---------- page ----------
def page(title, w, h, body):
    return '''<!doctype html>
<html lang="%s">
<head>
<meta charset="utf-8">
<title>%s</title>
<script src="./support.js"></script>
</head>
<body>
<x-dc>
<helmet>
%s
<style>
body{margin:0}
button{font:inherit;color:inherit;background:none;border:0;padding:0;cursor:pointer;text-align:inherit}
input{font:inherit;color:inherit;background:none;border:0;padding:0;margin:0;outline:0;min-width:0}
h1,h2,h3,p,ul,li{margin:0;padding:0;font-size:inherit;font-weight:inherit;list-style:none}
a{color:%s}a:hover{color:%s}
</style>
</helmet>
<div dir="%s" style="width: %dpx; height: %dpx; box-sizing: border-box; position: relative; overflow: hidden; background: %s; color: %s; font-family: %s; font-size: %s; line-height: %s">
%s
</div>
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{"$preview":{"width":%d,"height":%d}}'>
class Component extends DCLogic {
renderVals() {
return {};
}
}
</script>
</body>
</html>
''' % (cx.lang, plain(title), LINK, t.acctext, t.text, 'rtl' if cx.rtl else 'ltr', w, h, t.ground, t.text, FF_TEXT, f14, lh(), body, w, h)
boards = {}; order = []
def begin(name, theme='dark', rtl=True):
    cx.name = name; cx.theme = THEMES[theme]; cx.rtl = rtl; cx.lang = 'ar' if rtl else 'en'; cx.uid = 0
    CLICKS[name] = []
def out(title, w, h, body, canvas_title, row):
    """w, h in grid units."""
    W, H = w * BASE, h * BASE
    os.makedirs(ROOT, exist_ok=True)
    open(os.path.join(ROOT, cx.name), 'w', encoding='utf-8').write(page(title, W, H, body))
    boards[cx.name] = dict(w=W, h=H, title=canvas_title, row=row, theme=cx.theme.name, rtl=cx.rtl); order.append(cx.name)

# ---------- building blocks (the bricks of ultar-ui, section 3) ----------
FOCUS = lambda: 'outline: %s solid %s; outline-offset: %s' % (RING, t.focus, RING)
def h1(s):
    return '<h1 style="font-family: %s; font-size: %s; font-weight: %d">%s</h1>' % (FF_TITLE, f24, w7, tx(s))
def h2(s, size=None):
    return '<h2 style="font-family: %s; font-size: %s; font-weight: %d">%s</h2>' % (FF_TITLE, size or f16, w6, tx(s))
def sub(s, color=None):
    return '<div style="font-size: %s; color: %s">%s</div>' % (f12, color or t.mute, tx(s))
def note(s):
    """Marks a value the specification does not define."""
    return '<div style="font-size: %s; color: %s; display: flex; align-items: center; gap: %s">%s<span>%s</span></div>' % (f12, t.mute, s2, ic('info', True), tx(s))
NA = '—'
def _state_css(kind, state):
    bg, fg, bd = {'normal': (t.raised, t.text, t.ctl), 'primary': (t.acc, t.onacc, t.acctext), 'quiet': ('transparent', t.text, 'transparent'), 'danger': (t.raised, t.danger, t.danger)}[kind]
    bw = EDGE2 if kind == 'primary' else HAIR; extra = ''
    if state == 'hover':
        if kind == 'primary': bg = over(t.acc, t.onacc, _st['hover_overlay'])
        else: bg = t.hover
    elif state == 'pressed':
        if kind == 'primary': bg = over(t.acc, t.onacc, _st['pressed_overlay'])
        else: bg = t.line
    elif state == 'focus': extra = '; ' + FOCUS()
    elif state == 'disabled': extra = '; ' + DIM
    elif state == 'error': bd = t.danger; bw = RING
    return 'background: %s; color: %s; border: %s solid %s; box-sizing: border-box%s' % (bg, fg, bw, bd, extra)
def state_extra(state):
    """The focus ring or the disabled opacity of a brick that draws its own colours."""
    return '; ' + FOCUS() if state == 'focus' else ('; ' + DIM if state == 'disabled' else '')
def _aria_state(state):
    return {'disabled': ' aria-disabled="true"', 'loading': ' aria-busy="true"', 'error': ' aria-invalid="true"'}.get(state, '')
def button(label, a, kind='normal', state='', icon=None, name=None, grow=False):
    pre = ic('ring', True) if state == 'loading' else (ic('error', True) if state == 'error' else (ic(icon, True) if icon else ''))
    return '<button%s%s%s style="height: %s; padding: 0 %s; border-radius: %s; font-weight: %d; display: inline-flex; align-items: center; justify-content: center; gap: %s; white-space: nowrap; %s%s">%s<span>%s</span></button>' % (
        act(a, name or label), ' aria-label="%s"' % plain(name) if name else '', _aria_state(state), tgt, s4, r_ctl, w6, s2, 'flex: 1; ' if grow else '', _state_css(kind, state), pre, tx(label))
def ibtn(label, icon, a, active=False, state='', small=True, color=None, tab=True, big=False):
    size = touch if big else tgt
    css = _state_css('quiet', state)
    if active: css = css.replace('background: transparent', 'background: %s' % t.hover)
    return '<button%s aria-label="%s"%s%s style="width: %s; height: %s; flex: none; border-radius: %s; display: inline-flex; align-items: center; justify-content: center; %s%s">%s</button>' % (
        act(a, label), plain(label), _aria_state(state), '' if tab else ' tabindex="-1"', size, size, r_ctl, css, '; color: %s' % color if color else '', ic('ring' if state == 'loading' else icon, small))
def switch(on, label, a, state=''):
    # On: a track in accent_text (3:1 on every background) with a surface knob; off: an outline in control_border with a text knob.
    track = t.acctext if on else t.raised; bd = t.acctext if on else t.ctl; knob = t.surface if on else t.text
    layer = {'hover': 'hover_overlay', 'pressed': 'pressed_overlay'}.get(state)
    if layer: track = over(t.acctext, t.surface, _st[layer]) if on else (t.hover if state == 'hover' else t.line)
    if state == 'error': bd = t.danger
    bw = RING if state == 'error' else HAIR
    return '<button role="switch" aria-checked="%s" aria-label="%s"%s%s style="width: %s; height: %s; flex: none; display: inline-flex; align-items: center; border-radius: %s%s"><span style="width: %s; height: %s; border-radius: %s; background: %s; border: %s solid %s; box-sizing: border-box; display: flex; align-items: center; justify-content: %s; padding: 0 %s">%s</span></button>' % (
        'true' if on else 'false', plain(label), act(a, label), _aria_state(state), s7, tgt, r_round, state_extra(state), s7, ic_l, r_round, track, bw, bd, 'flex-end' if on else 'flex-start', s1,
        ic('ring', True, t.acctext if on else t.ctl) if state == 'loading' else '<span style="width: %s; height: %s; border-radius: %s; background: %s"></span>' % (ic_s, ic_s, r_round, knob))
def select(value, label, a, state=''):
    return '<button aria-haspopup="listbox" aria-label="%s: %s"%s%s style="height: %s; padding: 0 %s; border-radius: %s; display: inline-flex; align-items: center; justify-content: space-between; gap: %s; white-space: nowrap; %s"><span>%s</span>%s</button>' % (
        plain(label), plain(value), act(a, label), _aria_state(state), tgt, s3, r_ctl, s2, _state_css('normal', state), tx(value), ic('ring' if state == 'loading' else 'chevron-down', True))
def slider(label, pct, a, state='', icon=None, width=None):
    # Filled part and knob ring in accent_text, the rest in control_border (danger with an error); the knob in text.
    rest = t.danger if state == 'error' else t.ctl
    fill, knob, ring = t.acctext, t.text, RING
    pre = '<span style="display: inline-flex; color: %s">%s</span>' % (t.mute, ic(icon)) if icon else ''
    return '<div style="display: flex; align-items: center; gap: %s; %s">%s<div role="slider" tabindex="0" aria-label="%s" aria-orientation="horizontal" aria-valuemin="0" aria-valuemax="100" aria-valuenow="%d" aria-valuetext="%d%%"%s%s style="flex: 1; height: %s; display: flex; align-items: center; border-radius: %s%s"><span style="width: %d%%; height: %s; border-radius: %s; background: %s"></span><span style="width: %s; height: %s; flex: none; border-radius: %s; background: %s; border: %s solid %s; box-sizing: border-box"></span><span style="flex: 1; height: %s; border-radius: %s; background: %s"></span></div><span style="width: %s; text-align: end; font-size: %s; color: %s">%s</span></div>' % (
        s3, 'width: %s' % width if width else 'flex: 1', pre, plain(label), pct, pct, act(a, label), _aria_state(state), tgt, r_ctl, state_extra(state), pct, s1, r_round, fill, ic_s, ic_s, r_round, knob, ring, t.acctext, s1, r_round, rest, s7, f12, t.mute, ltr('%d%%' % pct))
def field(label, a, value='', hint='', kind='text', state='', error='', icon=None, width=None, show_label=True, direction=None):
    """Text field with a visible label and an error text. direction: fixed text direction (numbers, addresses)."""
    i = uid('f'); e = uid('e')
    css = _state_css('normal', 'focus' if state == 'focus' else ('disabled' if state == 'disabled' else ('error' if state == 'error' else ('hover' if state == 'hover' else ''))))
    if state == 'pressed': css = _state_css('normal', '').replace(HAIR + ' solid ' + t.ctl, RING + ' solid ' + t.text)
    lab = '<label for="%s" style="font-size: %s; font-weight: %d">%s</label>' % (i, f12, w5, tx(label)) if show_label else ''
    err = '<div id="%s" role="alert" style="font-size: %s; color: %s; display: flex; align-items: center; gap: %s">%s<span>%s</span></div>' % (e, f12, t.danger, s1, ic('error', True), tx(error)) if error else ''
    pre = '<span style="display: inline-flex; color: %s">%s</span>' % (t.mute, ic('ring' if state == 'loading' else icon, True)) if (icon or state == 'loading') else ''
    return '<div style="display: flex; flex-direction: column; gap: %s; %s">%s<div style="height: %s; padding: 0 %s; border-radius: %s; display: flex; align-items: center; gap: %s; %s">%s<input id="%s" type="%s"%s%s value="%s" placeholder="%s"%s%s%s%s style="flex: 1; height: 100%%" /></div>%s</div>' % (
        s1, 'width: %s' % width if width else 'flex: 1', lab, tgt, s3, r_ctl, s2, css, pre, i, kind, act(a, label), '' if show_label else ' aria-label="%s"' % plain(label), plain(value), plain(hint),
        ' dir="%s"' % direction if direction else '', ' disabled' if state == 'disabled' else '', ' aria-invalid="true"' if state == 'error' else '', ' aria-describedby="%s"' % e if error else '', err)
def checkrow(label, a, on, subtext='', state='', end='', icon=None, mixed=False):
    """Checkbox: the whole row is the target."""
    bd = t.acctext if on else t.ctl; bg = t.acc if on else t.raised; rowbg = 'transparent'
    layer = {'hover': 'hover_overlay', 'pressed': 'pressed_overlay'}.get(state)
    if layer: rowbg = t.hover if state == 'hover' else t.line; bg = over(t.acc, t.onacc, _st[layer]) if on else rowbg
    if state == 'error': bd = t.danger
    box = '<span style="width: %s; height: %s; flex: none; border-radius: %s; background: %s; border: %s solid %s; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; color: %s">%s</span>' % (BOX, BOX, r_ctl, bg, HAIR, bd, t.onacc, ic('ring', True, t.mute) if state == 'loading' else (ic('check', True) if on else ''))
    pre = '<span style="display: inline-flex; color: %s">%s</span>' % (t.mute, ic(icon)) if icon else ''
    return '<button role="checkbox" aria-checked="%s"%s%s style="width: 100%%; min-height: %s; padding: %s %s; box-sizing: border-box; border-radius: %s; display: flex; align-items: center; gap: %s; background: %s; color: %s%s">%s%s<span style="flex: 1; display: block"><span style="display: block; font-weight: %d">%s</span>%s</span>%s</button>' % (
        'mixed' if mixed else ('true' if on else 'false'), act(a, label), _aria_state(state), s7, s2, s3, r_ctl, s3, rowbg, t.text, state_extra(state), box, pre, w5, tx(label),
        '<span style="display: block; font-size: %s; color: %s">%s</span>' % (f12, t.mute, tx(subtext)) if subtext else '', end)
def radiocard(title, subtext, on, a, grow=True, body=''):
    dot = '<span style="width: %s; height: %s; flex: none; border-radius: %s; border: %s solid %s; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center">%s</span>' % (BOX, BOX, r_round, RING if on else HAIR, t.acctext if on else t.ctl, '<span style="width: %s; height: %s; border-radius: %s; background: %s"></span>' % (s2, s2, r_round, t.acctext) if on else '')
    return '<button role="radio" aria-checked="%s"%s%s style="%spadding: %s; border-radius: %s; background: %s; border: %s solid %s; box-sizing: border-box; display: flex; align-items: flex-start; gap: %s"><span style="display: inline-flex">%s</span><span style="flex: 1; display: block"><span style="display: block; font-weight: %d">%s</span><span style="display: block; font-size: %s; color: %s">%s</span>%s</span></button>' % (
        'true' if on else 'false', act(a, title), '' if on else ' tabindex="-1"', 'flex: 1; ' if grow else 'width: 100%; ', s3, r_card, t.hover if on else t.surface, RING if on else HAIR, t.acctext if on else t.ctl, s3, dot, w6, tx(title), f12, t.mute, tx(subtext), body)
def radiogroup(label, inner, column=False):
    return '<div role="radiogroup" aria-label="%s" style="display: flex; gap: %s%s">%s</div>' % (plain(label), s3, '; flex-direction: column' if column else '', inner)
def badge(s, role='mute', icon=None):
    col = {'mute': t.mute, 'info': t.info, 'ok': t.ok, 'warn': t.warn, 'danger': t.danger, 'acc': t.acctext}[role]
    return '<span style="display: inline-flex; align-items: center; gap: %s; height: %s; padding: 0 %s; border-radius: %s; border: %s solid %s; color: %s; font-size: %s; font-weight: %d; white-space: nowrap">%s<span>%s</span></span>' % (s1, ic_l, s2, r_ctl, HAIR, col, col, f12, w6, ic(icon, True) if icon else '', tx(s))
def hint(role, title, text='', actions='', icon=None):
    """Hint bar: role colour + icon + text (colour never alone)."""
    col, ico = {'info': (t.info, 'info'), 'ok': (t.ok, 'check'), 'warn': (t.warn, 'warning'), 'danger': (t.danger, 'error')}[role]; icon = icon or ico
    return '<div role="%s" style="border-radius: %s; background: %s; border: %s solid %s; padding: %s %s; display: flex; align-items: center; gap: %s"><span style="display: inline-flex; color: %s">%s</span><div style="flex: 1"><div style="font-weight: %d">%s</div>%s</div>%s</div>' % (
        'alert' if role in ('danger', 'warn') else 'status', r_card, t.raised, HAIR, col, s3, s4, s3, col, ic(icon), w6, tx(title), sub(text) if text else '', actions)
def card(inner, pad=None, grow=False, label=None):
    return '<section%s style="border-radius: %s; background: %s; border: %s solid %s; padding: %s; overflow: hidden%s">%s</section>' % (' aria-label="%s"' % plain(label) if label else '', r_card, t.surface, HAIR, t.line, pad or '0', '; flex: 1' if grow else '', inner)
def row(label, subtext, control, last=False, icon=None):
    pre = '<span style="display: inline-flex; color: %s">%s</span>' % (t.mute, ic(icon)) if icon else ''
    return '<div style="min-height: %s; padding: %s %s; box-sizing: border-box; display: flex; align-items: center; gap: %s; border-bottom: %s">%s<div style="flex: 1"><div style="font-weight: %d">%s</div>%s</div>%s</div>' % (ROW_MIN, s2, s4, s4, 'none' if last else '%s solid %s' % (HAIR, t.line), pre, w5, tx(label), sub(subtext) if subtext else '', control)
def cardhead(title, subtext='', end=''):
    return '<div style="padding: %s; display: flex; align-items: center; gap: %s; border-bottom: %s solid %s"><div style="flex: 1">%s%s</div>%s</div>' % (s4, s4, HAIR, t.line, h2(title), sub(subtext) if subtext else '', end)
def navitem(icon, label, a, active=False):
    """Sidebar item; the selection mark (accent_text, 4 x 16) at the end edge (approved round-2 board; the toolkit follows)."""
    mark = '<span style="width: %s; height: %s; flex: none; border-radius: %s; background: %s"></span>' % (s1, ic_s, r_round, t.acctext if active else 'transparent')
    return '<button%s%s style="width: 100%%; height: %s; flex: none; padding: 0 %s; box-sizing: border-box; border-radius: %s; display: flex; align-items: center; gap: %s; background: %s; font-weight: %d"><span style="display: inline-flex; color: %s">%s</span><span style="flex: 1">%s</span>%s</button>' % (
        act(a, label), ' aria-current="page"' if active else '', tgt, s3, r_ctl, s3, t.hover if active else 'transparent', w6 if active else w4, t.text if active else t.mute, ic(icon, True), tx(label), mark)
def sidebar(label, inner, width=70):
    return '<nav aria-label="%s" style="width: %s; flex: none; padding: %s; box-sizing: border-box; border-inline-end: %s solid %s; background: %s; display: flex; flex-direction: column; gap: %s; overflow-y: auto">%s</nav>' % (plain(label), g(width), s3, HAIR, t.line, t.bar, s1, inner)
def searchbox(label, a, value='', width=None, state=''):
    return field(label, a, value=value, hint=label, kind='search', icon='search', width=width, show_label=False, state=state)
def progress(label, pct):
    return '<div role="progressbar" aria-label="%s" aria-valuemin="0" aria-valuemax="100" aria-valuenow="%d" style="height: %s; border-radius: %s; background: %s; display: flex"><span style="width: %d%%; border-radius: %s; background: %s"></span></div>' % (plain(label), pct, s1, r_round, t.ctl, pct, r_round, t.acctext)
def busy(label):
    return '<div role="status" style="display: flex; flex-direction: column; align-items: center; justify-content: center; gap: %s; color: %s; flex: 1">%s<span>%s</span></div>' % (s3, t.mute, ic('ring'), tx(label))
def empty(icon, title, text, actions=''):
    return '<div style="flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: %s; text-align: center; padding: %s"><span style="display: inline-flex; color: %s">%s</span>%s<div style="color: %s">%s</div><div style="display: flex; gap: %s">%s</div></div>' % (s3, s5, t.mute, ic(icon), h2(title), t.mute, tx(text), s2, actions)
def key(s):
    """Keyboard key: always a left-to-right island (R20)."""
    return '<kbd dir="ltr" style="font-family: %s; font-size: %s; height: %s; padding: 0 %s; border-radius: %s; border: %s solid %s; display: inline-flex; align-items: center">%s</kbd>' % (FF_MONO, f12, ic_l, s2, r_ctl, HAIR, t.ctl, s)
def dialog(title, text, actions, width=120, live=''):
    """Dialog brick: hangs on its parent window and dims only that (section 5.2). First in the focus order."""
    i = uid('d'); j = uid('d')
    return '<div style="position: absolute; inset: 0; z-index: 3; background: %s; display: flex; align-items: center; justify-content: center"><div role="alertdialog" aria-modal="true" aria-labelledby="%s" aria-describedby="%s" style="width: %s; border-radius: %s; background: %s; border: %s solid %s; box-shadow: %s; padding: %s; box-sizing: border-box; display: flex; flex-direction: column; gap: %s"><h2 id="%s" style="font-family: %s; font-size: %s; font-weight: %d">%s</h2><div id="%s" style="color: %s">%s</div>%s<div style="display: flex; justify-content: flex-end; gap: %s">%s</div></div></div>' % (
        t.scrim, i, j, g(width), r_card, t.raised, HAIR, t.line, t.shadow, s5, s4, i, FF_TITLE, f16, w6, tx(title), j, t.mute, tx(text), live, s2, actions)

# ---------- shell ----------
AR = dict(overview='نظرة عامة', quick='الإعدادات السريعة', files='الملفات', browser='المتصفح', terminal='الطرفية', editor='محرر النصوص', store='متجر Ultar', settings='الإعدادات', trash='سلة المحذوفات', apps='كل التطبيقات', minimize='تصغير', maximize='تكبير', close='إغلاق', search='بحث', usb='ذاكرة USB', kbd='لغة الإدخال: العربية', kbds='ع', clock='16:52', date='الأحد 22 تشرين الثاني', cal='التقويم والإشعارات', bar='الشريط', phone='الهاتف')
EN = dict(overview='Overview', quick='Quick settings', files='Files', browser='Browser', terminal='Terminal', editor='Text editor', store='Ultar Store', settings='Settings', trash='Trash', apps='All apps', minimize='Minimize', maximize='Maximize', close='Close', search='Search', usb='USB drive', kbd='Input language: English', kbds='EN', clock='16:52', date='Sun 22 Nov', cal='Calendar and notifications', bar='Bar', phone='Phone')
def S():
    return AR if cx.rtl else EN
APPS = [('folder', 'files'), ('browser', 'browser'), ('terminal', 'terminal'), ('editor', 'editor'), ('bag', 'store'), ('gear', 'settings')]
def tile(icon, label, a, run=0, active=False):
    """Bar and dock tile: touch-sized target; dots below the icon = running windows (shape, not only colour)."""
    dots = ''.join('<span style="width: %s; height: %s; border-radius: %s; background: %s"></span>' % (s1, s1, r_round, t.acctext if run else 'transparent') for _ in range(max(run, 1)))
    return '<button%s aria-label="%s%s" style="width: %s; height: %s; flex: none; border-radius: %s; background: %s; border: %s solid %s; box-sizing: border-box; display: inline-flex; flex-direction: column; align-items: center; justify-content: center; gap: %s">%s<span style="display: flex; gap: %s">%s</span></button>' % (
        act(a, label), plain(label), L(' (يعمل)', ' (running)') if run else '', touch, touch, r_ctl, t.hover if active else t.raised, HAIR, t.ctl if active else t.line, s1, ic(icon), s1, dots)
def vsep():
    return '<span aria-hidden="true" style="width: %s; height: %s; background: %s"></span>' % (HAIR, ic_l, t.line)
def status_button(extra=(), pressed=False):
    icons = ''.join(ic(n, True) for n in ('wifi', 'volume', 'battery') + tuple(extra))
    return '<button%s aria-label="%s" aria-haspopup="dialog" aria-expanded="%s" style="height: %s; padding: 0 %s; border-radius: %s; display: inline-flex; align-items: center; gap: %s; background: %s">%s</button>' % (act('shell.status', S()['quick']), S()['quick'], 'true' if pressed else 'false', tgt, s2, r_ctl, s3, t.hover if pressed else 'transparent', icons)
def kbd_button():
    return '<button%s aria-label="%s" aria-haspopup="menu" style="height: %s; min-width: %s; padding: 0 %s; box-sizing: border-box; border-radius: %s; font-size: %s; font-weight: %d; color: %s; display: inline-flex; align-items: center; justify-content: center">%s</button>' % (act('shell.kbd', S()['kbd']), S()['kbd'], tgt, tgt, s2, r_ctl, f12, w6, t.mute, S()['kbds'])
def bottombar(running=(), active=None, usb=True, open_=None):
    """THE standard: one bar at the bottom (section 11). Start: trash and mounted drives. Centre: overview, all apps,
    search, pinned and running apps. End: input language, status, clock."""
    s = S()
    start = tile('bin', s['trash'], 'shell.trash') + (tile('usb', s['usb'], 'shell.drive') if usb else '')
    centre = '<button%s aria-label="%s" style="width: %s; height: %s; flex: none; border-radius: %s; display: inline-flex; align-items: center; justify-content: center; background: %s">%s</button>' % (act('shell.overview', s['overview']), s['overview'], touch, touch, r_ctl, t.hover if open_ == 'overview' else 'transparent', mark())
    centre += ibtn(s['apps'], 'grid', 'shell.apps', small=False, big=True)
    centre += '<button%s aria-label="%s" style="width: %s; height: %s; flex: none; padding: 0 %s; box-sizing: border-box; border-radius: %s; background: %s; border: %s solid %s; display: inline-flex; align-items: center; gap: %s; color: %s">%s<span>%s</span></button>' % (act('shell.search', s['search']), s['search'], g(50), tgt, s3, r_ctl, t.surface, HAIR, t.ctl, s2, t.mute, ic('search', True), s['search'])
    centre += ''.join(tile(i, s[k], 'shell.app', run=dict(running).get(k, 0), active=(k == active)) for i, k in APPS)
    end = kbd_button() + status_button(pressed=(open_ == 'quick'))
    end += '<button%s aria-label="%s: %s %s" aria-haspopup="dialog" aria-expanded="%s" style="height: %s; padding: 0 %s; border-radius: %s; text-align: end; font-size: %s; background: %s"><span style="display: block; font-weight: %d">%s</span><span style="display: block; color: %s">%s</span></button>' % (act('shell.clock', s['cal']), s['cal'], plain(s['date']), s['clock'], 'true' if open_ == 'calendar' else 'false', touch, s2, r_ctl, f12, t.hover if open_ == 'calendar' else 'transparent', w6, ltr(s['clock']), t.mute, tx(s['date']))
    return '<nav aria-label="%s" style="position: absolute; inset-inline: 0; bottom: 0; height: %s; background: %s; border-top: %s solid %s; box-sizing: border-box; display: flex; align-items: center; padding: 0 %s; z-index: 4"><div style="flex: 1; display: flex; align-items: center; gap: %s">%s</div><div style="display: flex; align-items: center; gap: %s">%s</div><div style="flex: 1; display: flex; justify-content: flex-end; align-items: center; gap: %s">%s</div></nav>' % (s['bar'], bar_h, t.bar, HAIR, t.line, s3, s2, start, s2, centre, s1, end)
def topbar(app):
    """Variant only: bar at the top (section 5.1), height = pointer target."""
    s = S()
    return '<nav aria-label="%s" style="position: absolute; inset-inline: 0; top: 0; height: %s; background: %s; border-bottom: %s solid %s; display: flex; align-items: center; padding: 0 %s; z-index: 4"><div style="flex: 1; display: flex; align-items: center; gap: %s"><button%s aria-label="%s" style="width: %s; height: %s; border-radius: %s; display: inline-flex; align-items: center; justify-content: center">%s</button>%s</div><button%s aria-haspopup="dialog" style="height: %s; padding: 0 %s; border-radius: %s; font-weight: %d">%s</button><div style="flex: 1; display: flex; align-items: center; justify-content: flex-end; gap: %s">%s%s</div></nav>' % (
        s['bar'], tgt, t.bar, HAIR, t.line, s2, s2, act('shell.overview', s['overview']), s['overview'], tgt, tgt, r_ctl, mark(True),
        '<button%s aria-haspopup="menu" style="height: %s; padding: 0 %s; border-radius: %s; font-weight: %d">%s</button>' % (act('shell.appmenu', app), tgt, s2, r_ctl, w6, tx(app)) if app else '',
        act('shell.clock', s['cal']), tgt, s3, r_ctl, w5, tx(s['date'] + ' · ' + s['clock']), s1, kbd_button(), status_button())
def dock(running=()):
    s = S()
    inner = ibtn(s['apps'], 'grid', 'shell.apps', small=False, big=True) + vsep() + ''.join(tile(i, s[k], 'shell.app', run=dict(running).get(k, 0)) for i, k in APPS) + vsep() + tile('usb', s['usb'], 'shell.drive') + tile('bin', s['trash'], 'shell.trash')
    return '<nav aria-label="%s" style="position: absolute; inset-inline: 0; bottom: %s; height: %s; display: flex; justify-content: center; z-index: 4"><div style="display: flex; align-items: center; gap: %s; padding: 0 %s; border-radius: %s; background: %s; border: %s solid %s; box-sizing: border-box; box-shadow: %s">%s</div></nav>' % (L('المرسى', 'Dock'), s3, bar_h, s2, s2, r_card, t.bar, HAIR, t.line, t.shadow, inner)
def titlebar(title, extra=''):
    """Drawn by the compositor: title in the centre, buttons at the END (decision 3). Not in the Tab order (A37):
    keyboard reaches them through the window menu and shortcuts (section 5.2)."""
    s = S()
    b = ibtn(s['minimize'], 'min', 'win.min', tab=False) + ibtn(s['maximize'], 'max', 'win.max', tab=False) + ibtn(s['close'], 'close', 'win.close', tab=False)
    return '<header style="height: %s; flex: none; display: flex; align-items: center; padding: 0 %s; border-bottom: %s solid %s; background: %s; box-sizing: border-box"><div style="flex: 1; display: flex; align-items: center; gap: %s">%s</div><h1 style="font-family: %s; font-weight: %d">%s</h1><div style="flex: 1; display: flex; justify-content: flex-end; gap: %s">%s</div></header>' % (title_h, s1, HAIR, t.line, t.bar, s1, extra, FF_TITLE, w6, tx(title), s1, b)
def window(x, y, w, h, title, body, extra='', z=1):
    """Floating window; x, y, w, h in grid units. x is measured from the start side."""
    return '<section aria-label="%s" style="position: absolute; inset-inline-start: %s; top: %s; width: %s; height: %s; z-index: %d; border-radius: %s; overflow: hidden; background: %s; border: %s solid %s; box-sizing: border-box; box-shadow: %s; display: flex; flex-direction: column">%s%s</section>' % (plain(title), g(x), g(y), g(w), g(h), z, r_card, t.surface, HAIR, t.line, t.shadow, titlebar(title, extra), body)
def appwindow(title, body, extra='', overlay=''):
    """A maximised app window that fills the board."""
    return '<section aria-label="%s" style="position: absolute; inset: 0; display: flex; flex-direction: column; background: %s">%s%s<div style="flex: 1; display: flex; min-height: 0; position: relative">%s</div></section>' % (plain(title), t.ground, titlebar(title, extra), '', overlay + body)
def sheet(label, inner, width, side='end', bottom=True):
    """Floating sheet of the shell (quick settings, calendar): opens from the bar at the end side."""
    return '<section role="dialog" aria-label="%s" style="position: absolute; %s: %s; inset-inline-end: %s; width: %s; z-index: 5; border-radius: %s; background: %s; border: %s solid %s; box-shadow: %s; padding: %s; box-sizing: border-box; display: flex; flex-direction: column; gap: %s">%s</section>' % (plain(label), 'bottom' if bottom else 'top', g(16), s2, g(width), r_card, t.surface, HAIR, t.line, t.shadow, s4, s4, inner)
